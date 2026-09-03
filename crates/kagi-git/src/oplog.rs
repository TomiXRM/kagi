//! Operation log — T017
//!
//! Appends structured JSON Lines records to `$KAGI_LOG_DIR/operations.jsonl`
//! (or `$HOME/.kagi/operations.jsonl` if `KAGI_LOG_DIR` is not set).
//!
//! The file is created (and its parent directory auto-created) on first write.
//! Write failures are reported to stderr only — they never abort the application.
//!
//! JSON serialisation is hand-written to avoid adding a `serde` dependency.
//! Every string field passes through [`escape_json_string`] which escapes
//! `"`, `\`, and control characters (`\n`, `\r`, `\t` and U+0000–U+001F).
//!
//! # Public API
//!
//! - [`OpOutcome`] — operation result variant
//! - [`OpLogEntry`] — one log record
//! - [`append_oplog`] — write `entry` to the JSONL file

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ops::StateSummary, GitError};

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// The result of a git operation.
#[derive(Debug, Clone)]
pub enum OpOutcome {
    /// Operation completed without error.
    Success {
        /// Repository state immediately after execution.
        after: StateSummary,
    },
    /// Operation was only PARTIALLY applied: the working tree was mutated but
    /// did not fully succeed (issue #281). The recovery handle (e.g. discard's
    /// backup blob SHAs) lives in `after.dirty`.
    Partial {
        /// Repository state immediately after the partial execution.
        after: StateSummary,
        /// Human-readable description of what failed.
        error: String,
    },
    /// Operation failed (preflight failure, execute error, etc.).
    Failed {
        /// Human-readable error description.
        error: String,
    },
    /// Operation was refused because blockers were present at plan time.
    Refused {
        /// The blocker strings that prevented execution.
        blockers: Vec<String>,
    },
}

/// Who initiated an operation (ADR-0149 / #333). Serialized as the lowercase
/// strings `human` / `mcp` / `cli`. Defaults to [`Actor::Human`] — including
/// for pre-ADR-0149 log lines that predate the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Actor {
    /// A person driving the GUI.
    #[default]
    Human,
    /// The MCP server (agent-facing write path).
    Mcp,
    /// The `kagi` CLI.
    Cli,
}

impl Actor {
    /// Wire form written to / read from the JSONL file.
    pub fn as_str(&self) -> &'static str {
        match self {
            Actor::Human => "human",
            Actor::Mcp => "mcp",
            Actor::Cli => "cli",
        }
    }

    /// Parse the wire form; anything unrecognized (or missing) falls back to
    /// [`Actor::Human`] so old / malformed lines still load.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "mcp" => Actor::Mcp,
            "cli" => Actor::Cli,
            _ => Actor::Human,
        }
    }
}

/// One entry in the operation log.
#[derive(Debug, Clone)]
pub struct OpLogEntry {
    /// Monotonic sequence id, assigned at append time (ADR-0149 / #333). Gives
    /// a total order even for entries recorded in the same wall-clock second.
    /// For pre-ADR-0149 lines that lack the field, [`read_oplog_tail`]
    /// reconstructs it from the 0-based index of the entry in the file.
    pub id: u64,
    /// Id of the previous entry (`None` for the first). Chains the log so a
    /// future selective-undo can walk backwards. Reconstructed for old lines.
    pub parent: Option<u64>,
    /// Unix epoch seconds at the time the operation was recorded.
    pub timestamp: i64,
    /// Operation name: `"checkout"`, `"create-branch"`, `"stash-push"`,
    /// `"stash-apply"`, or `"cherry-pick"`.
    pub op: String,
    /// Absolute path to the repository working tree.
    pub repo: String,
    /// Who initiated the operation. Defaults to [`Actor::Human`].
    pub actor: Actor,
    /// Absolute path to the worktree the operation ran in (`None` for old
    /// lines that predate the field).
    pub worktree: Option<String>,
    /// Repository state captured at plan time (before execution).
    pub before: StateSummary,
    /// Outcome of the operation.
    pub outcome: OpOutcome,
}

impl OpLogEntry {
    /// Construct a new entry with `timestamp` set to the current wall time.
    ///
    /// `id`/`parent` are placeholders (`0` / `None`) here — [`append_oplog`]
    /// assigns the real sequence id from the file's tail at write time.
    /// `actor` defaults to [`Actor::Human`]; `worktree` to `None`. Use
    /// [`OpLogEntry::with_actor`] / [`OpLogEntry::with_worktree`] to set them.
    pub fn new(
        op: impl Into<String>,
        repo: impl Into<String>,
        before: StateSummary,
        outcome: OpOutcome,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        OpLogEntry {
            id: 0,
            parent: None,
            timestamp,
            op: op.into(),
            repo: repo.into(),
            actor: Actor::Human,
            worktree: None,
            before,
            outcome,
        }
    }

    /// Builder: set the initiating actor.
    pub fn with_actor(mut self, actor: Actor) -> Self {
        self.actor = actor;
        self
    }

    /// Builder: set the worktree path.
    pub fn with_worktree(mut self, worktree: Option<String>) -> Self {
        self.worktree = worktree;
        self
    }
}

// ────────────────────────────────────────────────────────────
// JSON serialisation helpers
// ────────────────────────────────────────────────────────────

/// Escape a string for embedding in JSON: wrap in `"` and escape
/// `\`, `"`, `\n`, `\r`, `\t`, and remaining control characters.
///
/// This is the only place where string values enter the JSON output.
fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Encode remaining control chars as \uXXXX.
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Serialise a [`StateSummary`] as a JSON object string.
fn state_summary_to_json(s: &StateSummary) -> String {
    format!(
        "{{\"head\":{},\"dirty\":{}}}",
        escape_json_string(&s.head),
        escape_json_string(&s.dirty),
    )
}

/// Serialise an [`OpLogEntry`] as a single-line JSON object (no trailing newline).
pub fn entry_to_json(entry: &OpLogEntry) -> String {
    let outcome_json = match &entry.outcome {
        OpOutcome::Success { after } => {
            format!(
                "{{\"kind\":\"Success\",\"after\":{}}}",
                state_summary_to_json(after)
            )
        }
        OpOutcome::Partial { after, error } => {
            format!(
                "{{\"kind\":\"Partial\",\"after\":{},\"error\":{}}}",
                state_summary_to_json(after),
                escape_json_string(error)
            )
        }
        OpOutcome::Failed { error } => {
            format!(
                "{{\"kind\":\"Failed\",\"error\":{}}}",
                escape_json_string(error)
            )
        }
        OpOutcome::Refused { blockers } => {
            let blocker_strs: Vec<String> =
                blockers.iter().map(|b| escape_json_string(b)).collect();
            format!(
                "{{\"kind\":\"Refused\",\"blockers\":[{}]}}",
                blocker_strs.join(",")
            )
        }
    };

    // ADR-0149: `parent` / `worktree` serialize as JSON `null` when absent.
    let parent_json = match entry.parent {
        Some(p) => p.to_string(),
        None => "null".to_string(),
    };
    let worktree_json = match &entry.worktree {
        Some(w) => escape_json_string(w),
        None => "null".to_string(),
    };

    format!(
        "{{\"id\":{},\"parent\":{},\"timestamp\":{},\"op\":{},\"repo\":{},\"actor\":{},\"worktree\":{},\"before\":{},\"outcome\":{}}}",
        entry.id,
        parent_json,
        entry.timestamp,
        escape_json_string(&entry.op),
        escape_json_string(&entry.repo),
        escape_json_string(entry.actor.as_str()),
        worktree_json,
        state_summary_to_json(&entry.before),
        outcome_json,
    )
}

// ────────────────────────────────────────────────────────────
// File path resolution
// ────────────────────────────────────────────────────────────

/// Resolve the path to `operations.jsonl`.
///
/// Priority:
/// 1. `$KAGI_LOG_DIR/operations.jsonl` — if the env var is set (used by tests
///    and CI to avoid writing to `$HOME`).
/// 2. `$HOME/.kagi/operations.jsonl` — default production path.
///
/// Returns `None` if neither `$KAGI_LOG_DIR` nor `$HOME` can be determined.
fn log_file_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("operations.jsonl"));
        }
    }
    // Fall back to $HOME/.kagi/operations.jsonl.
    dirs_home().map(|home| home.join(".kagi").join("operations.jsonl"))
}

/// Minimal home-directory resolution without adding a crate dependency.
///
/// Tries `$HOME` (Unix) then `$USERPROFILE` (Windows).
pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// Minimal hand-written JSON parser (T-BP-004)
// ────────────────────────────────────────────────────────────
//
// Parses ONLY the format produced by `entry_to_json` above.
// This is NOT a general JSON parser — it rejects any line it cannot
// fully understand and the caller skips that line (fail-safe).
//
// Supported escapes (matching `escape_json_string`): \" \\ \n \r \t \uXXXX.
// All other sequences are passed through unchanged (they should not appear
// in well-formed output, but skipping them beats panicking).

/// Unescape a JSON string value that was produced by `escape_json_string`.
///
/// `s` must NOT include the surrounding `"` delimiters.
fn unescape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
        } else {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    // Consume exactly 4 hex digits.
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(code) {
                            out.push(c);
                        }
                    }
                }
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => {}
            }
        }
    }
    out
}

/// Extract the string value for a simple `"key":"value"` or `"key":number` pair
/// from a flat JSON fragment.  Returns the raw (unescaped) string for string
/// values, or the decimal text for integer values.
///
/// Only searches within `json` — does NOT recurse into nested objects.
/// Returns `None` if the key is not found or parsing fails.
fn extract_str_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let pos = json.find(needle.as_str())?;
    let after = json[pos + needle.len()..].trim_start();

    if after.starts_with('"') {
        // String value: scan for the closing (unescaped) '"'.
        let inner_start = 1; // skip opening '"'
        let mut escaped = false;
        let mut end = None;
        for (i, ch) in after[inner_start..].char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                end = Some(inner_start + i);
                break;
            }
        }
        let end = end?;
        Some(unescape_json_str(&after[inner_start..end]))
    } else {
        // Number or other scalar: read until `,`, `}`, or end.
        let end = after.find([',', '}']).unwrap_or(after.len());
        let val = after[..end].trim();
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    }
}

/// Extract the JSON object substring starting right after `"key":` in `json`.
///
/// Scans forward until the matching `}` at depth 0, skipping nested objects.
fn extract_object_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let pos = json.find(needle.as_str())?;
    let after = json[pos + needle.len()..].trim_start();
    if !after.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escape = false;
    let mut end = None;
    for (i, ch) in after.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_str {
            match ch {
                '\\' => escape = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1); // include closing '}'
                    break;
                }
            }
            _ => {}
        }
    }
    Some(after[..end?].to_string())
}

/// Extract the JSON array of strings under `"key":[...]` from `json`.
///
/// Returns only the string elements; other element types are skipped.
fn extract_string_array(json: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{}\":[", key);
    let pos = match json.find(needle.as_str()) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let after = &json[pos + needle.len()..];

    // Scan elements until the closing ']'.
    let mut result = Vec::new();
    let mut rest = after;
    loop {
        let rest_t = rest.trim_start();
        if rest_t.starts_with(']') || rest_t.is_empty() {
            break;
        }
        if let Some(inner) = rest_t.strip_prefix('"') {
            // String element: find end.
            let mut escaped = false;
            let mut end = None;
            for (i, ch) in inner.char_indices() {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end = Some(i);
                    break;
                }
            }
            if let Some(e) = end {
                result.push(unescape_json_str(&inner[..e]));
                rest = &inner[e + 1..]; // skip past closing '"'
                                        // Skip optional comma.
                rest = rest.trim_start();
                if rest.starts_with(',') {
                    rest = &rest[1..];
                }
            } else {
                break;
            }
        } else {
            // Non-string token: skip to next comma or ']'.
            let skip = rest_t.find([',', ']']).unwrap_or(rest_t.len());
            rest = &rest_t[skip..];
            if rest.starts_with(',') {
                rest = &rest[1..];
            }
        }
    }
    result
}

/// Parse a single JSONL line produced by `entry_to_json`.
///
/// Returns `None` if any required field is missing or malformed.
/// Malformed but non-critical fields (e.g. before.dirty) receive empty defaults.
fn parse_oplog_line(line: &str) -> Option<OpLogEntry> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }

    // Top-level fields.
    let timestamp: i64 = extract_str_field(line, "timestamp")?.parse().ok()?;
    let op = extract_str_field(line, "op")?;
    let repo = extract_str_field(line, "repo")?;

    // ADR-0149 fields. Absent (pre-ADR-0149 lines) → id/parent are fixed up by
    // `read_oplog_tail`; actor defaults to Human; worktree stays None.
    // A literal `null` scalar decodes to None. (A worktree path literally named
    // "null" would be misread as None — accepted edge; paths are absolute.)
    let id: u64 = extract_str_field(line, "id")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let parent: Option<u64> = match extract_str_field(line, "parent") {
        Some(s) if s != "null" => s.parse().ok(),
        _ => None,
    };
    let actor = extract_str_field(line, "actor")
        .map(|s| Actor::from_wire(&s))
        .unwrap_or_default();
    let worktree: Option<String> = match extract_str_field(line, "worktree") {
        Some(s) if s != "null" => Some(s),
        _ => None,
    };

    // "before" object.
    let before_obj = extract_object_field(line, "before")?;
    let before_head = extract_str_field(&before_obj, "head").unwrap_or_default();
    let before_dirty = extract_str_field(&before_obj, "dirty").unwrap_or_default();
    let before = super::ops::StateSummary {
        head: before_head,
        dirty: before_dirty,
    };

    // "outcome" object.
    let outcome_obj = extract_object_field(line, "outcome")?;
    let kind = extract_str_field(&outcome_obj, "kind")?;

    let outcome = match kind.as_str() {
        "Success" => {
            let after_obj = extract_object_field(&outcome_obj, "after")?;
            let head = extract_str_field(&after_obj, "head").unwrap_or_default();
            let dirty = extract_str_field(&after_obj, "dirty").unwrap_or_default();
            OpOutcome::Success {
                after: super::ops::StateSummary { head, dirty },
            }
        }
        "Partial" => {
            let after_obj = extract_object_field(&outcome_obj, "after")?;
            let head = extract_str_field(&after_obj, "head").unwrap_or_default();
            let dirty = extract_str_field(&after_obj, "dirty").unwrap_or_default();
            let error = extract_str_field(&outcome_obj, "error").unwrap_or_default();
            OpOutcome::Partial {
                after: super::ops::StateSummary { head, dirty },
                error,
            }
        }
        "Failed" => {
            let error = extract_str_field(&outcome_obj, "error").unwrap_or_default();
            OpOutcome::Failed { error }
        }
        "Refused" => {
            let blockers = extract_string_array(&outcome_obj, "blockers");
            OpOutcome::Refused { blockers }
        }
        _ => return None,
    };

    Some(OpLogEntry {
        id,
        parent,
        timestamp,
        op,
        repo,
        actor,
        worktree,
        before,
        outcome,
    })
}

/// Read the last `n` entries from the oplog file (newest last in file,
/// returned newest-first by reversing the tail slice).
///
/// Uses the same path resolution as [`append_oplog`] (`$KAGI_LOG_DIR` first,
/// then `$HOME/.kagi/operations.jsonl`).
///
/// Lines that cannot be parsed are silently skipped.
/// Returns an empty `Vec` if the file does not exist or cannot be read.
pub fn read_oplog_tail(n: usize) -> Vec<OpLogEntry> {
    let path = match log_file_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Parse oldest-first, reconstructing id/parent for pre-ADR-0149 lines that
    // lack an explicit `id`: id = 0-based index of the entry in the file,
    // parent = previous entry's id. New lines carry their own id/parent.
    let mut entries: Vec<OpLogEntry> = Vec::new();
    let mut prev_id: Option<u64> = None;
    let mut idx: u64 = 0;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(mut entry) = parse_oplog_line(line) {
            if !line.contains("\"id\":") {
                entry.id = idx;
                entry.parent = prev_id;
            }
            prev_id = Some(entry.id);
            idx += 1;
            entries.push(entry);
        }
    }

    // Return the tail (up to n), newest first.
    let start = entries.len().saturating_sub(n);
    entries[start..].iter().rev().cloned().collect()
}

/// Append `entry` to the operation log file as a JSON Lines record.
///
/// The parent directory is created automatically if it does not exist.
/// Any I/O failure is printed to stderr and returned as a [`GitError`] so
/// the caller can log it — but the caller is **expected to ignore this error**
/// and let the application continue normally.
///
/// Returns the path of the file that was written to on success.
pub fn append_oplog(entry: &OpLogEntry) -> Result<PathBuf, GitError> {
    use std::io::Write;

    let path = log_file_path().ok_or_else(|| {
        GitError::Other("could not determine oplog path (no HOME or KAGI_LOG_DIR)".to_string())
    })?;

    // Auto-create parent directory.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!(
                "oplog: mkdir failed for {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    // ADR-0149: assign the sequence id/parent from the current tail so ids are
    // monotonic and each entry chains to the previous one. Placeholder id/parent
    // on `entry` (from `OpLogEntry::new`) are overwritten here.
    // ponytail: single-user oplog — no cross-process locking on the read→write
    // window; add a lock if concurrent writers ever appear.
    let mut entry = entry.clone();
    let last = read_oplog_tail(1);
    match last.first() {
        Some(prev) => {
            entry.id = prev.id.saturating_add(1);
            entry.parent = Some(prev.id);
        }
        None => {
            entry.id = 0;
            entry.parent = None;
        }
    }

    let line = format!("{}\n", entry_to_json(&entry));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| {
            GitError::Other(format!("oplog: open failed for {}: {}", path.display(), e))
        })?;

    file.write_all(line.as_bytes()).map_err(|e| {
        GitError::Other(format!("oplog: write failed for {}: {}", path.display(), e))
    })?;

    Ok(path)
}

// ────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────

// Unit tests live in a child file to keep this file under the LOC ratchet
// (escape/serialize/parse/append coverage).
#[cfg(test)]
#[path = "oplog_tests.rs"]
mod tests;

// ADR-0129 Phase 1 — oplog on-disk compatibility tests live in a child file
// (`oplog_adr0129_tests.rs`) to keep this file under the LOC ratchet.
#[cfg(test)]
#[path = "oplog_adr0129_tests.rs"]
mod adr0129_compat_tests;
