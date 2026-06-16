//! Smart commit message domain logic — pure data and string transforms.
//!
//! The git2 staged-diff collection and Ollama HTTP calls live in the git-backend
//! layer (`kagi::git::message_gen`) and re-export these items.

use std::path::Path;

use crate::status::{ChangeKind, FileStatus};

// ──────────────────────────────────────────────────────────────────────────
// Tunables
// ──────────────────────────────────────────────────────────────────────────

/// Maximum number of bytes of staged diff sent to the LLM.  Larger diffs are
/// truncated to this prefix and a file summary is appended (ADR-0044: ~8 KB).
pub const DIFF_TRUNCATE_BYTES: usize = 8 * 1024;

/// Default Ollama host (loopback only — ADR-0044).
pub const DEFAULT_OLLAMA_HOST: &str = "localhost:11434";

// ──────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────

/// Which backend generates the message.
///
/// Plain enum dispatch (ADR-0044): no trait.  `External { .. }` is intentionally
/// absent — remote APIs are out of MVP scope and require a separate ADR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageBackend {
    /// Local Ollama server (loopback).  `host` is `host:port`, `model` an
    /// installed model name (from `/api/tags`).
    Ollama {
        /// `host:port`, e.g. `localhost:11434`.
        host: String,
        /// Installed model name, e.g. `gemma:2b`.
        model: String,
    },
    /// Deterministic rule-based fallback.  Always available, always non-empty.
    RuleBased,
}

/// Output language for the generated message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Japanese.
    Ja,
    /// English.
    En,
}

impl Lang {
    /// Stable lowercase slug used in `settings.json`.
    pub fn slug(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }

    /// Parse a slug (`"ja"` / `"en"`), defaulting to `En`.
    pub fn from_slug(s: &str) -> Lang {
        match s {
            "ja" => Lang::Ja,
            _ => Lang::En,
        }
    }
}

/// Message style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `type(scope): summary` (Conventional Commits).
    ConventionalCommits,
    /// Plain free-form summary.
    Plain,
}

impl Style {
    /// Stable lowercase slug used in `settings.json`.
    pub fn slug(self) -> &'static str {
        match self {
            Style::ConventionalCommits => "conventional",
            Style::Plain => "plain",
        }
    }

    /// Parse a slug, defaulting to `ConventionalCommits`.
    pub fn from_slug(s: &str) -> Style {
        match s {
            "plain" => Style::Plain,
            _ => Style::ConventionalCommits,
        }
    }
}

/// Input to [`generate_message`] / [`rule_based`].
#[derive(Debug, Clone)]
pub struct GenInput {
    /// The (already truncated) staged diff text.
    pub diff: String,
    /// Output language.
    pub lang: Lang,
    /// Output style.
    pub style: Style,
    /// Ask the model for a subject **and a short body** (used by template mode,
    /// whose body field would otherwise stay empty). When false, subject only
    /// (ADR-0090).
    pub want_body: bool,
}

/// Failure modes of [`generate_message`] for the Ollama backend.
///
/// The caller treats every variant the same way: fall back quietly to the
/// rule-based draft (ADR-0044).  The variants exist for logging / tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// `KAGI_OFFLINE=1` — no network call was attempted.
    Offline,
    /// The HTTP request failed, timed out, or returned a non-2xx status.
    Http(String),
    /// The reply could not be parsed or carried no usable `response`.
    EmptyResponse,
    /// There was nothing staged to generate from.
    NoStagedChanges,
}

impl std::fmt::Display for GenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenError::Offline => write!(f, "offline (KAGI_OFFLINE=1)"),
            GenError::Http(m) => write!(f, "http error: {}", m),
            GenError::EmptyResponse => write!(f, "empty LLM response"),
            GenError::NoStagedChanges => write!(f, "no staged changes"),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Diff truncation / file summary
// ──────────────────────────────────────────────────────────────────────────

/// Truncate `diff` to [`DIFF_TRUNCATE_BYTES`] at a line boundary and append a
/// truncation marker + file summary when truncation occurred.  Pure function so
/// it can be unit-tested without a repo.
pub fn truncate_with_summary(diff: &str, files: &[FileStatus]) -> String {
    if diff.len() <= DIFF_TRUNCATE_BYTES {
        return diff.to_string();
    }

    // Cut at the last newline within the budget so we never split a UTF-8
    // sequence or a diff line in half.  `char_indices` keeps us UTF-8 safe.
    let mut cut = 0usize;
    for (i, c) in diff.char_indices() {
        if i >= DIFF_TRUNCATE_BYTES {
            break;
        }
        if c == '\n' {
            cut = i + 1;
        }
    }
    if cut == 0 {
        // No newline in range: hard-cut at the last char boundary ≤ budget.
        cut = diff
            .char_indices()
            .take_while(|(i, _)| *i < DIFF_TRUNCATE_BYTES)
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0);
    }

    let mut out = String::with_capacity(cut + 256);
    out.push_str(&diff[..cut]);
    out.push_str("\n[... diff truncated for length ...]\n");
    out.push_str(&file_summary(files));
    out
}

/// One-line-per-file `A/M/D` summary, e.g. `Files: A src/a.rs, M src/b.rs`.
pub fn file_summary(files: &[FileStatus]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = files
        .iter()
        .map(|f| format!("{} {}", change_letter(&f.change), f.path.display()))
        .collect();
    format!("Files: {}\n", parts.join(", "))
}

/// Single-letter change code (`A`/`M`/`D`/`R`/`T`).
fn change_letter(change: &ChangeKind) -> char {
    match change {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
        ChangeKind::Renamed { .. } => 'R',
        ChangeKind::TypeChange => 'T',
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Rule-based generation (pure, always non-empty)
// ──────────────────────────────────────────────────────────────────────────

/// Conventional Commit type inferred from a staged file set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitType {
    Feat,
    Fix,
    Test,
    Docs,
    Chore,
}

impl CommitType {
    fn as_str(self) -> &'static str {
        match self {
            CommitType::Feat => "feat",
            CommitType::Fix => "fix",
            CommitType::Test => "test",
            CommitType::Docs => "docs",
            CommitType::Chore => "chore",
        }
    }
}

/// Classify a single path into a coarse category for type inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Test,
    Docs,
    Config,
    Code,
}

/// Classify a path by its name / extension.  Pure, `chars()`-safe.
fn classify_path(path: &Path) -> PathKind {
    let full = path.to_string_lossy().to_lowercase();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Tests: a `tests/` directory, a `_test`/`.test`/`.spec` suffix, or a
    // `test_` prefix.
    let stem = name.split('.').next().unwrap_or(&name);
    if full.contains("/tests/")
        || full.starts_with("tests/")
        || full.contains("/test/")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.starts_with("test_")
        || name.contains(".test.")
        || name.contains(".spec.")
    {
        return PathKind::Test;
    }

    // Docs: markdown / rst / txt, or a `docs/` directory.
    if matches!(ext.as_str(), "md" | "mdx" | "rst" | "txt" | "adoc")
        || full.contains("/docs/")
        || full.starts_with("docs/")
        || name == "readme"
        || name == "license"
    {
        return PathKind::Docs;
    }

    // Config / chore: build & config files, lockfiles, CI, dotfiles.
    let config_exts = [
        "toml", "yaml", "yml", "json", "ini", "cfg", "conf", "lock", "env", "mk",
    ];
    let config_names = [
        "cargo.toml",
        "cargo.lock",
        "package.json",
        "makefile",
        "dockerfile",
        ".gitignore",
        ".gitattributes",
    ];
    if config_exts.contains(&ext.as_str())
        || config_names.contains(&name.as_str())
        || full.contains("/.github/")
        || name.starts_with('.')
    {
        return PathKind::Config;
    }

    PathKind::Code
}

/// Infer the Conventional Commit type from the staged file set.
fn infer_type(files: &[FileStatus]) -> CommitType {
    if files.is_empty() {
        return CommitType::Chore;
    }
    let kinds: Vec<PathKind> = files.iter().map(|f| classify_path(&f.path)).collect();
    let all = |k: PathKind| kinds.iter().all(|&x| x == k);

    if all(PathKind::Test) {
        return CommitType::Test;
    }
    if all(PathKind::Docs) {
        return CommitType::Docs;
    }
    if all(PathKind::Config) {
        return CommitType::Chore;
    }

    // Mixed / code: an all-deletes change leans `chore`; a pure-add of code
    // leans `feat`; otherwise the conservative default is `feat` for new code
    // and `fix` only when every change is a modification of existing code.
    let any_added = files.iter().any(|f| matches!(f.change, ChangeKind::Added));
    let all_deleted = files
        .iter()
        .all(|f| matches!(f.change, ChangeKind::Deleted));
    let all_modified = files
        .iter()
        .all(|f| matches!(f.change, ChangeKind::Modified));

    if all_deleted {
        CommitType::Chore
    } else if any_added {
        CommitType::Feat
    } else if all_modified {
        CommitType::Fix
    } else {
        CommitType::Feat
    }
}

/// Derive a `scope` from the staged files' common top-level directory, if any.
fn infer_scope(files: &[FileStatus]) -> Option<String> {
    let first_seg = |f: &FileStatus| -> Option<String> {
        let p = f.path.to_string_lossy();
        let seg = p.split('/').next()?;
        // A bare filename (no dir, `seg == p`) or an empty segment has no scope.
        if seg == p || seg.is_empty() {
            None
        } else {
            Some(seg.to_string())
        }
    };
    let mut iter = files.iter();
    let first = first_seg(iter.next()?)?;
    for f in iter {
        if first_seg(f).as_deref() != Some(first.as_str()) {
            return None;
        }
    }
    Some(first)
}

/// Short display name for a file (basename).
fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Build the rule-based commit message.  **Pure, deterministic, never empty.**
///
/// Single file → `<type>: <verb> <file>` (verb from add/delete/modify).
/// Multiple files → `<type>(<scope>): update N files` (scope = common dir).
/// Honours `style` (Conventional vs Plain) and `lang` (Ja vs En) for the vocab.
pub fn rule_based(input: &GenInput, files: &[FileStatus]) -> String {
    let ct = infer_type(files);

    // Build the bare summary (no type prefix yet).
    let summary = if files.is_empty() {
        // Defensive: should not happen in practice (caller stages first), but
        // the contract is "never empty".
        match input.lang {
            Lang::Ja => "変更をコミット".to_string(),
            Lang::En => "update changes".to_string(),
        }
    } else if files.len() == 1 {
        let f = &files[0];
        let name = base_name(&f.path);
        match (input.lang, &f.change) {
            (Lang::En, ChangeKind::Added) => format!("add {}", name),
            (Lang::En, ChangeKind::Deleted) => format!("remove {}", name),
            (Lang::En, ChangeKind::Renamed { .. }) => format!("rename {}", name),
            (Lang::En, _) => format!("update {}", name),
            (Lang::Ja, ChangeKind::Added) => format!("{} を追加", name),
            (Lang::Ja, ChangeKind::Deleted) => format!("{} を削除", name),
            (Lang::Ja, ChangeKind::Renamed { .. }) => format!("{} をリネーム", name),
            (Lang::Ja, _) => format!("{} を更新", name),
        }
    } else {
        let n = files.len();
        match input.lang {
            Lang::En => format!("update {} files", n),
            Lang::Ja => format!("{} ファイルを更新", n),
        }
    };

    match input.style {
        Style::Plain => {
            // Plain: capitalise-ish, no type prefix.  For Ja leave as-is.
            summary
        }
        Style::ConventionalCommits => {
            let scope = if files.len() > 1 {
                infer_scope(files)
            } else {
                None
            };
            match scope {
                Some(s) => format!("{}({}): {}", ct.as_str(), s, summary),
                None => format!("{}: {}", ct.as_str(), summary),
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Ollama JSON helpers
// ──────────────────────────────────────────────────────────────────────────

/// Build the JSON request body for `/api/generate` by hand (no serde).
///
/// `{ "model": "...", "prompt": "...", "stream": false }` with `model` and
/// `prompt` JSON-escaped.
pub fn ollama_generate_request_body(model: &str, prompt: &str, think: Option<bool>) -> String {
    // Low temperature for deterministic, format-adherent commit subjects;
    // `num_predict` bounds output. `think` (when Some) sets Ollama's reasoning
    // toggle: for a *thinking* model we send `think:false` so it answers the
    // subject directly instead of spending the whole budget reasoning (which
    // yields an empty `response`). Non-thinking models may reject the field, so
    // the caller retries with `None` on failure (ADR-0090).
    let think_field = match think {
        Some(true) => "\"think\":true,",
        Some(false) => "\"think\":false,",
        None => "",
    };
    format!(
        "{{\"model\":\"{}\",\"prompt\":\"{}\",\"stream\":false,{}\
         \"options\":{{\"temperature\":0.2,\"top_p\":0.9,\"num_predict\":128}}}}",
        json_escape(model),
        json_escape(prompt),
        think_field
    )
}

/// JSON-escape a string for embedding in a double-quoted JSON value.
/// Handles the characters that MUST be escaped per RFC 8259.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Extract the `"response"` string value from an Ollama `/api/generate` reply
/// (handwritten JSON scan, no serde).  Decodes the common JSON escapes so the
/// returned message is human-readable.  Returns `None` when absent / empty.
pub fn parse_ollama_response(json: &str) -> Option<String> {
    let key_pos = json.find("\"response\"")?;
    let after = &json[key_pos + "\"response\"".len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let val = read_json_string(rest)?;
    if val.trim().is_empty() {
        None
    } else {
        Some(val)
    }
}

/// Parse a `/api/tags` reply into the list of installed model names
/// (handwritten JSON scan, no serde).
///
/// The reply shape is `{ "models": [ { "name": "...", ... }, ... ] }`.  We scan
/// every `"name"` key and collect its string value, preserving order and
/// de-duplicating.  Returns an empty `Vec` when none are found.
pub fn parse_ollama_tags(json: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut from = 0usize;
    let key = "\"name\"";
    while let Some(rel) = json[from..].find(key) {
        let key_pos = from + rel;
        let after = &json[key_pos + key.len()..];
        from = key_pos + key.len();
        let Some(colon) = after.find(':') else {
            continue;
        };
        let rest = &after[colon + 1..];
        if let Some(name) = read_json_string(rest) {
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Read a JSON string value that begins (after optional whitespace) at the start
/// of `s`.  Decodes `\"`, `\\`, `\n`, `\r`, `\t`, `\/`, `\b`, `\f`, and `\uXXXX`
/// escapes.  Returns `None` if the value is not a string (e.g. `null`).
/// `char_indices`-based for UTF-8 safety.
fn read_json_string(s: &str) -> Option<String> {
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    if chars.peek() != Some(&'"') {
        return None;
    }
    chars.next(); // opening quote

    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{08}'),
                'f' => out.push('\u{0c}'),
                'u' => {
                    let mut code = 0u32;
                    for _ in 0..4 {
                        let h = chars.next()?;
                        code = code * 16 + h.to_digit(16)?;
                    }
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    }
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Tidy an LLM message: strip surrounding code fences / quotes / whitespace and
/// keep only the first non-empty line as the subject (MVP is single-line).
pub fn clean_llm_message(raw: &str) -> String {
    let trimmed = raw.trim();
    // Strip a wrapping ``` fence if present.
    let body = if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop the first line (``` or ```text) and a trailing fence.
        let rest = rest.split_once('\n').map(|x| x.1).unwrap_or("");
        rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };
    // First non-empty line is the subject.
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    // Strip surrounding quotes a model sometimes adds.
    line.trim_matches('"').trim().to_string()
}

/// Tidy an LLM message but **keep the body** (subject + blank line + body).
/// Used when `want_body` is set (template mode). Strips a wrapping code fence
/// and a one-line preamble (e.g. "Here is the commit message:"), then trims.
pub fn clean_llm_message_multiline(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    // 1. Drop a single leading preamble line if the model added one despite the
    //    "output only" instruction (e.g. "Here is the commit message:").
    if let Some((first, rest)) = text.split_once('\n') {
        let low = first.trim().to_lowercase();
        let is_preamble = low.ends_with(':')
            && (low.contains("commit message")
                || low.starts_with("here")
                || low.starts_with("sure"));
        if is_preamble {
            text = rest.trim_start().to_string();
        }
    }
    // 2. Strip a wrapping ``` fence (now possibly the leading line).
    if let Some(rest) = text.trim().strip_prefix("```") {
        let rest = rest.split_once('\n').map(|x| x.1).unwrap_or("");
        text = rest
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(rest)
            .trim()
            .to_string();
    }
    text.trim().to_string()
}

// ──────────────────────────────────────────────────────────────────────────
// Unit tests (pure logic)
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

    #[test]
    fn clean_multiline_keeps_body() {
        let raw = "feat: add x\n\n- did a\n- did b";
        assert_eq!(
            clean_llm_message_multiline(raw),
            "feat: add x\n\n- did a\n- did b"
        );
        // Fenced + preamble are stripped, body kept.
        let raw2 = "Here is the commit message:\n```\nfix: y\n\nbody line\n```";
        assert_eq!(clean_llm_message_multiline(raw2), "fix: y\n\nbody line");
    }

    #[test]
    fn rule_based_never_empty_even_with_no_files() {
        let en = rule_based(&input(Lang::En, Style::Plain), &[]);
        assert!(!en.is_empty());
        let ja = rule_based(&input(Lang::Ja, Style::ConventionalCommits), &[]);
        assert!(!ja.is_empty());
    }

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
        assert_eq!(msg, "chore: remove old.rs");
    }

    #[test]
    fn rule_based_single_modify_ja() {
        let files = vec![fs("src/main.rs", ChangeKind::Modified)];
        let msg = rule_based(&input(Lang::Ja, Style::Plain), &files);
        assert_eq!(msg, "main.rs を更新");
    }

    #[test]
    fn rule_based_multi_file_with_common_scope() {
        let files = vec![
            fs("src/a.rs", ChangeKind::Modified),
            fs("src/b.rs", ChangeKind::Added),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert_eq!(msg, "feat(src): update 2 files");
    }

    #[test]
    fn rule_based_multi_file_no_common_scope() {
        let files = vec![
            fs("src/a.rs", ChangeKind::Modified),
            fs("lib/b.rs", ChangeKind::Modified),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert_eq!(msg, "fix: update 2 files");
    }

    #[test]
    fn truncate_short_diff_unchanged() {
        let diff = "diff --git a/x b/x\n+hello\n";
        let files = vec![fs("x", ChangeKind::Modified)];
        assert_eq!(truncate_with_summary(diff, &files), diff);
    }

    #[test]
    fn truncate_long_diff_adds_marker_and_summary() {
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
    fn request_body_escapes_quotes_and_newlines() {
        let body = ollama_generate_request_body("gemma", "say \"hi\"\nnow", Some(false));
        assert!(body.contains("\\\"hi\\\""));
        assert!(body.contains("\\n"));
        assert!(body.contains("\"stream\":false"));
        assert!(body.contains("\"model\":\"gemma\""));
        assert!(body.contains("\"think\":false"));
        assert!(body.contains("\"temperature\":0.2"));
        // `think:None` omits the field entirely (for the no-think retry).
        let body2 = ollama_generate_request_body("gemma", "x", None);
        assert!(!body2.contains("think"));
    }

    #[test]
    fn clean_strips_fences_and_quotes() {
        assert_eq!(clean_llm_message("```\nfeat: add x\n```"), "feat: add x");
        assert_eq!(clean_llm_message("\"feat: add x\""), "feat: add x");
        assert_eq!(clean_llm_message("feat: add x\n\nbody text"), "feat: add x");
        assert_eq!(clean_llm_message("   "), "");
    }

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
