//! Smart Commit Message generation — W14-SMART (T-COMMIT-015, ADR-0044)
//!
//! This is the *backend* half of the Smart Commit Message feature.  It turns a
//! repository's **staged** diff into a candidate commit message via one of two
//! backends, dispatched by a plain `enum` (no trait — YAGNI, mirrors the avatar
//! resolver in `src/ui/avatar_fetch.rs`):
//!
//!   * [`MessageBackend::RuleBased`] — a pure, dependency-free function that
//!     derives a plain commit message from the staged file set.  It is **always
//!     available** and **always returns a non-empty string**.  No network, no
//!     LLM, fully deterministic.
//!   * [`MessageBackend::Ollama`] — POSTs the (truncated) staged diff to a local
//!     Ollama server (`http://<host>/api/generate`) and extracts the `response`
//!     field from the JSON reply.  Used **only** when the user has explicitly
//!     enabled it and pressed "Generate with Local LLM".  On any failure,
//!     timeout, or `KAGI_OFFLINE=1`, it returns `Err` and the *caller* falls
//!     back quietly to the rule-based draft (ADR-0044).
//!
//! ## Privacy / network policy (ADR-0037 / ADR-0044)
//!
//!   * Only the **staged** diff is ever collected ([`collect_staged_diff`]).
//!     Unstaged / untracked changes are **never** included.
//!   * The staged diff leaves the process **only** for `MessageBackend::Ollama`,
//!     whose destination is a loopback `localhost` Ollama server.  External API
//!     backends are out of scope (a separate ADR is required).
//!   * `KAGI_OFFLINE=1` disables every network call so headless / fixture runs
//!     are deterministic and nothing escapes.
//!   * The diff is truncated to ~8 KB before being sent, with a file summary
//!     appended, to bound latency and token usage.
//!
//! HTTP reuses **ureq 3** (same blocking + global-timeout pattern as
//! `avatar_fetch.rs`); no new dependency is added.  JSON is parsed by hand (no
//! `serde`), mirroring `oplog.rs` / `avatar_fetch.rs`.

use std::path::Path;
use std::time::Duration;

use git2::{DiffOptions, Repository};

use super::status::{ChangeKind, FileStatus};
use super::{resolve_head, Head};

// ──────────────────────────────────────────────────────────────────────────
// Tunables
// ──────────────────────────────────────────────────────────────────────────

/// Maximum number of bytes of staged diff sent to the LLM.  Larger diffs are
/// truncated to this prefix and a file summary is appended (ADR-0044: ~8 KB).
pub const DIFF_TRUNCATE_BYTES: usize = 8 * 1024;

/// Global timeout for a single Ollama HTTP request.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Default Ollama host (loopback only — ADR-0044).
pub const DEFAULT_OLLAMA_HOST: &str = "localhost:11434";

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
            let cleaned = clean_llm_message(&raw);
            if cleaned.is_empty() {
                Err(GenError::EmptyResponse)
            } else {
                Ok(cleaned)
            }
        }
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
    let all_deleted = files.iter().all(|f| matches!(f.change, ChangeKind::Deleted));
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
// Ollama prompt + HTTP + JSON parse
// ──────────────────────────────────────────────────────────────────────────

/// Build the LLM prompt: instruction (lang/style aware) + truncated diff +
/// file summary.
fn build_prompt(input: &GenInput, files: &[FileStatus]) -> String {
    let style_line = match input.style {
        Style::ConventionalCommits => {
            "Use the Conventional Commits format: type(scope): summary."
        }
        Style::Plain => "Use a concise plain one-line summary.",
    };
    let lang_line = match input.lang {
        Lang::Ja => "Write the commit message in Japanese.",
        Lang::En => "Write the commit message in English.",
    };
    let summary = file_summary(files);
    format!(
        "Summarise the following staged git diff and produce ONE commit message.\n\
         {style_line} {lang_line} Keep the subject under 72 characters. \
         Respond with ONLY the commit message, no explanation, no code fences.\n\n\
         {summary}\n\
         --- staged diff ---\n{}\n--- end diff ---\n",
        input.diff
    )
}

/// POST a generation request to a local Ollama server and return the raw
/// `response` string.
///
/// `host` is `host:port` (e.g. `localhost:11434`).  Uses ureq 3 with a global
/// timeout (same pattern as `avatar_fetch.rs`).  Returns `Err(GenError::Http)`
/// on transport / status failure and `Err(GenError::EmptyResponse)` when the
/// reply has no `response` field.
fn ollama_generate(host: &str, model: &str, prompt: &str) -> Result<String, GenError> {
    let url = format!("http://{}/api/generate", host);
    let body = ollama_generate_request_body(model, prompt);

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

/// Build the JSON request body for `/api/generate` by hand (no serde).
///
/// `{ "model": "...", "prompt": "...", "stream": false }` with `model` and
/// `prompt` JSON-escaped.
pub fn ollama_generate_request_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"prompt\":\"{}\",\"stream\":false}}",
        json_escape(model),
        json_escape(prompt)
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
        let Some(colon) = after.find(':') else { continue };
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
        assert_eq!(models, vec!["gemma:2b".to_string(), "llama3:8b".to_string()]);
    }

    #[test]
    fn parse_ollama_tags_empty() {
        assert!(parse_ollama_tags(r#"{"models":[]}"#).is_empty());
        assert!(parse_ollama_tags("").is_empty());
    }

    // ── request body escaping ─────────────────────────────────

    #[test]
    fn request_body_escapes_quotes_and_newlines() {
        let body = ollama_generate_request_body("gemma", "say \"hi\"\nnow");
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
        assert_eq!(
            clean_llm_message("feat: add x\n\nbody text"),
            "feat: add x"
        );
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
