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

use super::{GitError, ops::StateSummary};

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
            '"'  => out.push_str("\\\""),
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
            let blocker_strs: Vec<String> = blockers.iter().map(|b| escape_json_string(b)).collect();
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
            GitError::Other(format!("oplog: mkdir failed for {}: {}", parent.display(), e))
        })?;
    }

    let line = format!("{}\n", entry_to_json(entry));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| GitError::Other(format!("oplog: open failed for {}: {}", path.display(), e)))?;

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
        assert!(json.contains("\"head\":\"branch: main\""), "before.head missing");
        assert!(json.contains("\"head\":\"branch: feature\""), "after.head missing");
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
        assert!(json.contains("Working tree has changes"), "blocker 1 missing");
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
            before: StateSummary { head: "branch: main".to_string(), dirty: "clean".to_string() },
            outcome: OpOutcome::Success {
                after: StateSummary { head: "branch: main".to_string(), dirty: "clean".to_string() },
            },
        };
        let json = entry_to_json(&entry);
        // repo path with special chars must be properly escaped.
        assert!(json.contains("\\\"quotes\\\""), "double-quote escaping failed");
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
        assert!(lines[0].contains("checkout"), "first line should mention checkout");
        assert!(lines[1].contains("create-branch"), "second line should mention create-branch");

        // Restore env.
        match prev {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None    => std::env::remove_var("KAGI_LOG_DIR"),
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

        assert!(line.contains("\"timestamp\":9999"),    "timestamp field");
        assert!(line.contains("\"op\":\"stash-apply\""), "op field");
        assert!(line.contains("\"repo\":\"/my/repo\""), "repo field");
        assert!(line.contains("\"kind\":\"Refused\""),  "outcome kind");
        assert!(line.contains("Working tree is dirty"), "blocker text");
        assert!(line.contains("\"head\":\"branch: feat\""), "before.head");
        assert!(line.contains("\"dirty\":\"2 modified\""),  "before.dirty");

        match prev {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None    => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
