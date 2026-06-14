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

/// One entry in the operation log.
#[derive(Debug, Clone)]
pub struct OpLogEntry {
    /// Unix epoch seconds at the time the operation was recorded.
    pub timestamp: i64,
    /// Operation name: `"checkout"`, `"create-branch"`, `"stash-push"`,
    /// `"stash-apply"`, or `"cherry-pick"`.
    pub op: String,
    /// Absolute path to the repository working tree.
    pub repo: String,
    /// Repository state captured at plan time (before execution).
    pub before: StateSummary,
    /// Outcome of the operation.
    pub outcome: OpOutcome,
}

impl OpLogEntry {
    /// Construct a new entry with `timestamp` set to the current wall time.
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
            timestamp,
            op: op.into(),
            repo: repo.into(),
            before,
            outcome,
        }
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
fn entry_to_json(entry: &OpLogEntry) -> String {
    let outcome_json = match &entry.outcome {
        OpOutcome::Success { after } => {
            format!(
                "{{\"kind\":\"Success\",\"after\":{}}}",
                state_summary_to_json(after)
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

    format!(
        "{{\"timestamp\":{},\"op\":{},\"repo\":{},\"before\":{},\"outcome\":{}}}",
        entry.timestamp,
        escape_json_string(&entry.op),
        escape_json_string(&entry.repo),
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
fn dirs_home() -> Option<PathBuf> {
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
        timestamp,
        op,
        repo,
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

    // Collect the last `n` non-empty parseable lines (file is oldest-first).
    let entries: Vec<OpLogEntry> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_oplog_line)
        .collect();

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

    let line = format!("{}\n", entry_to_json(entry));

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

#[cfg(test)]
mod tests {
    use super::*;

    // ── escape_json_string ────────────────────────────────────

    #[test]
    fn escape_plain_string() {
        assert_eq!(escape_json_string("hello"), "\"hello\"");
    }

    #[test]
    fn escape_double_quote() {
        assert_eq!(escape_json_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_json_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn escape_newline() {
        assert_eq!(escape_json_string("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn escape_carriage_return() {
        assert_eq!(escape_json_string("a\rb"), "\"a\\rb\"");
    }

    #[test]
    fn escape_tab() {
        assert_eq!(escape_json_string("a\tb"), "\"a\\tb\"");
    }

    #[test]
    fn escape_null_byte() {
        assert_eq!(escape_json_string("a\x00b"), "\"a\\u0000b\"");
    }

    #[test]
    fn escape_all_specials_together() {
        // "a\b"<newline>  →  "\"a\\\\b\\\"\\n\""
        let input = "a\\b\"\n";
        let result = escape_json_string(input);
        assert_eq!(result, "\"a\\\\b\\\"\\n\"");
    }

    // ── entry_to_json ─────────────────────────────────────────

    #[test]
    fn json_success_entry_contains_required_fields() {
        let entry = OpLogEntry {
            timestamp: 1_000_000,
            op: "checkout".to_string(),
            repo: "/tmp/repo".to_string(),
            before: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
            outcome: OpOutcome::Success {
                after: StateSummary {
                    head: "branch: feature".to_string(),
                    dirty: "clean".to_string(),
                },
            },
        };
        let json = entry_to_json(&entry);
        assert!(json.contains("\"timestamp\":1000000"), "timestamp missing");
        assert!(json.contains("\"op\":\"checkout\""), "op missing");
        assert!(json.contains("\"repo\":\"/tmp/repo\""), "repo missing");
        assert!(json.contains("\"kind\":\"Success\""), "kind missing");
        assert!(
            json.contains("\"head\":\"branch: main\""),
            "before.head missing"
        );
        assert!(
            json.contains("\"head\":\"branch: feature\""),
            "after.head missing"
        );
    }

    #[test]
    fn json_refused_entry_contains_blockers() {
        let entry = OpLogEntry {
            timestamp: 2_000_000,
            op: "checkout".to_string(),
            repo: "/tmp/repo".to_string(),
            before: StateSummary {
                head: "branch: main".to_string(),
                dirty: "1 modified".to_string(),
            },
            outcome: OpOutcome::Refused {
                blockers: vec![
                    "Working tree has changes".to_string(),
                    "Branch 'x' does not exist".to_string(),
                ],
            },
        };
        let json = entry_to_json(&entry);
        assert!(json.contains("\"kind\":\"Refused\""), "kind missing");
        assert!(
            json.contains("Working tree has changes"),
            "blocker 1 missing"
        );
        assert!(json.contains("Branch"), "blocker 2 missing");
    }

    #[test]
    fn json_failed_entry_contains_error() {
        let entry = OpLogEntry {
            timestamp: 3_000_000,
            op: "stash-push".to_string(),
            repo: "/tmp/repo".to_string(),
            before: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
            outcome: OpOutcome::Failed {
                error: "stash push failed: some error".to_string(),
            },
        };
        let json = entry_to_json(&entry);
        assert!(json.contains("\"kind\":\"Failed\""), "kind missing");
        assert!(json.contains("stash push failed"), "error text missing");
    }

    #[test]
    fn json_escapes_special_chars_in_repo_path() {
        let entry = OpLogEntry {
            timestamp: 0,
            op: "checkout".to_string(),
            repo: "/path/with \"quotes\" and \\backslash".to_string(),
            before: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
            outcome: OpOutcome::Success {
                after: StateSummary {
                    head: "branch: main".to_string(),
                    dirty: "clean".to_string(),
                },
            },
        };
        let json = entry_to_json(&entry);
        // repo path with special chars must be properly escaped.
        assert!(
            json.contains("\\\"quotes\\\""),
            "double-quote escaping failed"
        );
        assert!(json.contains("\\\\backslash"), "backslash escaping failed");
    }

    // ── append_oplog (integration-style, uses tempdir) ────────
    //
    // These tests manipulate the KAGI_LOG_DIR environment variable, which is
    // process-global.  We serialise them with a mutex so parallel test threads
    // do not interfere with each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn append_two_entries_creates_two_jsonl_lines() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let log_dir = dir.path().to_str().unwrap().to_string();

        // Temporarily override the env var for this test.
        let prev = std::env::var("KAGI_LOG_DIR").ok();
        std::env::set_var("KAGI_LOG_DIR", &log_dir);

        let make_entry = |op: &str, ts: i64| OpLogEntry {
            timestamp: ts,
            op: op.to_string(),
            repo: "/tmp/testrepo".to_string(),
            before: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
            outcome: OpOutcome::Success {
                after: StateSummary {
                    head: "branch: main".to_string(),
                    dirty: "clean".to_string(),
                },
            },
        };

        let path1 = append_oplog(&make_entry("checkout", 1)).expect("first write");
        let path2 = append_oplog(&make_entry("create-branch", 2)).expect("second write");
        assert_eq!(path1, path2, "both writes should go to the same file");

        let content = std::fs::read_to_string(&path1).expect("read log");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "expected 2 JSON lines, got: {:?}", lines);

        // Each line must contain the op name.
        assert!(
            lines[0].contains("checkout"),
            "first line should mention checkout"
        );
        assert!(
            lines[1].contains("create-branch"),
            "second line should mention create-branch"
        );

        // Restore env.
        match prev {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }

    #[test]
    fn append_includes_expected_json_fields() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let log_dir = dir.path().to_str().unwrap().to_string();

        let prev = std::env::var("KAGI_LOG_DIR").ok();
        std::env::set_var("KAGI_LOG_DIR", &log_dir);

        let entry = OpLogEntry {
            timestamp: 9_999,
            op: "stash-apply".to_string(),
            repo: "/my/repo".to_string(),
            before: StateSummary {
                head: "branch: feat".to_string(),
                dirty: "2 modified".to_string(),
            },
            outcome: OpOutcome::Refused {
                blockers: vec!["Working tree is dirty".to_string()],
            },
        };

        let path = append_oplog(&entry).expect("write");
        let line = std::fs::read_to_string(&path).expect("read");
        let line = line.trim_end();

        assert!(line.contains("\"timestamp\":9999"), "timestamp field");
        assert!(line.contains("\"op\":\"stash-apply\""), "op field");
        assert!(line.contains("\"repo\":\"/my/repo\""), "repo field");
        assert!(line.contains("\"kind\":\"Refused\""), "outcome kind");
        assert!(line.contains("Working tree is dirty"), "blocker text");
        assert!(line.contains("\"head\":\"branch: feat\""), "before.head");
        assert!(line.contains("\"dirty\":\"2 modified\""), "before.dirty");

        match prev {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
