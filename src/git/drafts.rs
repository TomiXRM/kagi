//! Commit-message draft autosave — T-COMMIT-007 (ADR-0042).
//!
//! Persists a work-in-progress commit message **per repository + branch** so it
//! survives an application restart, and clears it once the commit succeeds.
//!
//! Storage follows the oplog / avatar-cache conventions of this codebase:
//!
//! - Location: `$KAGI_LOG_DIR/drafts/` when the env var is set (used by tests
//!   to stay deterministic and never touch the user's `$HOME`), otherwise
//!   `$HOME/.kagi/drafts/`.
//! - One draft = one file, named `<sha1(repo_path + "\0" + branch)>.json`.
//!   Including `repo_path` keeps the same branch name in two different repos
//!   from colliding.
//! - Format: **hand-written JSON** (no `serde`, matching the project policy).
//!   String fields are escaped/unescaped with the same rules as the oplog
//!   writer (`"`, `\`, `\n`, `\r`, `\t`, and `\uXXXX` for other control chars).
//!
//! Reads are deliberately lenient: a missing or corrupt file is treated as "no
//! draft" so a broken file can never block a commit. Saving an empty (trimmed)
//! message deletes the file instead of leaving an empty draft behind.
//!
//! # Public API
//!
//! - [`Draft`] — one decoded draft record
//! - [`save_draft`] — write (or delete, when empty) the draft for a branch
//! - [`load_draft`] — read the draft for a branch (`None` when absent/corrupt)
//! - [`clear_draft`] — delete the draft for a branch (e.g. after a commit)

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::GitError;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// A decoded commit-message draft for a single repository + branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Absolute path to the repository working tree the draft belongs to.
    pub repo: String,
    /// Short branch name the draft belongs to, e.g. `"main"`.
    pub branch: String,
    /// The work-in-progress commit message (template drafts store the
    /// already-expanded plain text — see ADR-0042).
    pub message: String,
    /// Editor mode the message was authored in: `"plain"` or `"template"`.
    pub mode: String,
    /// Unix epoch seconds at the time the draft was last written.
    pub updated: u64,
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Save the draft `message` for `branch` in the repository at `repo_path`.
///
/// If `message` is empty after trimming, the draft file is **deleted** instead
/// (an empty draft is never persisted). `mode` is stored verbatim (typically
/// `"plain"` or `"template"`).
///
/// # Errors
///
/// Returns [`GitError::Other`] when the draft directory cannot be determined
/// (no `KAGI_LOG_DIR` and no `HOME`), or when the file write / delete fails.
pub fn save_draft(
    repo_path: &Path,
    branch: &str,
    message: &str,
    mode: &str,
) -> Result<(), GitError> {
    // Empty (trimmed) message → remove any existing draft, don't persist empties.
    if message.trim().is_empty() {
        return clear_draft(repo_path, branch);
    }

    let path = draft_file_path(repo_path, branch).ok_or_else(|| {
        GitError::Other("draft: could not determine drafts dir (no HOME or KAGI_LOG_DIR)".to_string())
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!("draft: mkdir failed for {}: {}", parent.display(), e))
        })?;
    }

    let repo_str = repo_path.to_string_lossy();
    let updated = now_unix();
    let json = draft_to_json(&repo_str, branch, message, mode, updated);

    std::fs::write(&path, json.as_bytes()).map_err(|e| {
        GitError::Other(format!("draft: write failed for {}: {}", path.display(), e))
    })?;

    Ok(())
}

/// Load the draft for `branch` in the repository at `repo_path`.
///
/// Returns `None` when no draft exists, the file cannot be read, or the JSON is
/// corrupt — a broken draft must never prevent the user from committing.
pub fn load_draft(repo_path: &Path, branch: &str) -> Option<Draft> {
    let path = draft_file_path(repo_path, branch)?;
    let content = std::fs::read_to_string(&path).ok()?;
    parse_draft_json(&content)
}

/// Delete the draft for `branch` in the repository at `repo_path`.
///
/// Succeeds silently when the file does not exist (a no-op clear is not an
/// error — e.g. clearing after a commit when no draft was ever saved).
///
/// # Errors
///
/// Returns [`GitError::Other`] when the drafts dir cannot be determined, or when
/// deleting an existing file fails for a reason other than "not found".
pub fn clear_draft(repo_path: &Path, branch: &str) -> Result<(), GitError> {
    let path = draft_file_path(repo_path, branch).ok_or_else(|| {
        GitError::Other("draft: could not determine drafts dir (no HOME or KAGI_LOG_DIR)".to_string())
    })?;

    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GitError::Other(format!(
            "draft: delete failed for {}: {}",
            path.display(),
            e
        ))),
    }
}

// ────────────────────────────────────────────────────────────
// Path resolution
// ────────────────────────────────────────────────────────────

/// Resolve the drafts directory.
///
/// Priority (mirrors the oplog path resolution):
/// 1. `$KAGI_LOG_DIR/drafts/` when `KAGI_LOG_DIR` is set and non-empty.
/// 2. `$HOME/.kagi/drafts/` otherwise.
///
/// Returns `None` if neither `$KAGI_LOG_DIR` nor `$HOME`/`$USERPROFILE` is set.
fn drafts_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("drafts"));
        }
    }
    dirs_home().map(|home| home.join(".kagi").join("drafts"))
}

/// Minimal home-directory resolution (no crate dependency): `$HOME` then
/// `$USERPROFILE`.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Full path to the draft file for `repo_path` + `branch`.
///
/// The filename is `<sha1(repo_path + "\0" + branch)>.json`. Using the NUL
/// separator keeps the (repo, branch) key unambiguous even if a path or branch
/// name contained the literal text of the other.
fn draft_file_path(repo_path: &Path, branch: &str) -> Option<PathBuf> {
    let key = format!("{}\0{}", repo_path.to_string_lossy(), branch);
    let name = format!("{}.json", sha1_hex(key.as_bytes()));
    drafts_dir().map(|dir| dir.join(name))
}

/// Current wall-clock time in Unix epoch seconds (0 on a clock error).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// Hand-written JSON (serde-free; same escaping as oplog.rs)
// ────────────────────────────────────────────────────────────

/// Escape a string for embedding in JSON: escapes `\`, `"`, `\n`, `\r`, `\t`,
/// and remaining control characters as `\uXXXX`. Does NOT add surrounding
/// quotes (the caller wraps the value).
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Reverse of [`escape_json`]: decode `\" \\ \n \r \t \uXXXX`. Unknown escape
/// sequences are passed through (they should not occur in our own output).
fn unescape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
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
    out
}

/// Render a draft as a single-line JSON object (matching the ADR-0042 schema).
fn draft_to_json(repo: &str, branch: &str, message: &str, mode: &str, updated: u64) -> String {
    format!(
        "{{\"repo\":\"{}\",\"branch\":\"{}\",\"message\":\"{}\",\"mode\":\"{}\",\"updated\":{}}}",
        escape_json(repo),
        escape_json(branch),
        escape_json(message),
        escape_json(mode),
        updated,
    )
}

/// Extract the string value for `"key":"…"` from a flat JSON fragment, honoring
/// backslash escapes. Returns the **unescaped** value, or `None` if absent.
fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let pos = json.find(needle.as_str())?;
    let after = &json[pos + needle.len()..];

    // Scan to the closing unescaped '"'.
    let mut escaped = false;
    let mut end = None;
    for (i, ch) in after.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            end = Some(i);
            break;
        }
    }
    end.map(|e| unescape_json(&after[..e]))
}

/// Extract the integer value for `"key":<number>` from a flat JSON fragment.
fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\":", key);
    let pos = json.find(needle.as_str())?;
    let after = json[pos + needle.len()..].trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

/// Parse a draft JSON object produced by [`draft_to_json`].
///
/// Returns `None` when the required `branch` / `message` fields are missing or
/// the input is not a JSON object — corrupt drafts are treated as "no draft".
/// `repo` defaults to empty and `mode` to `"plain"` if absent (lenient read).
fn parse_draft_json(content: &str) -> Option<Draft> {
    let content = content.trim();
    if !content.starts_with('{') {
        return None;
    }
    // branch + message are the load-bearing fields; without them there is no
    // usable draft.
    let branch = extract_string(content, "branch")?;
    let message = extract_string(content, "message")?;
    let repo = extract_string(content, "repo").unwrap_or_default();
    let mode = extract_string(content, "mode").unwrap_or_else(|| "plain".to_string());
    let updated = extract_u64(content, "updated").unwrap_or(0);

    Some(Draft {
        repo,
        branch,
        message,
        mode,
        updated,
    })
}

// ────────────────────────────────────────────────────────────
// Self-contained SHA-1 (no crate dependency)
// ────────────────────────────────────────────────────────────
//
// The draft filename key is specified as `sha1(repo_path + "\0" + branch)` by
// ADR-0042 / T-COMMIT-007. `Cargo.toml` is frozen and ships no sha1 crate, so a
// small self-contained implementation is used purely as a stable filename hash
// (no security properties are relied upon).

/// Compute the SHA-1 digest of `data` and render it as a 40-char lowercase hex
/// string. Self-contained (RFC 3174); used only as a stable filename key.
fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    // Pre-processing: append 0x80, pad with zeros to 56 mod 64, then the
    // 64-bit big-endian bit length.
    let ml: u64 = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999_u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = String::with_capacity(40);
    for half in [h0, h1, h2, h3, h4] {
        out.push_str(&format!("{:08x}", half));
    }
    out
}

// ────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── sha1 known-answer (locks the filename key format) ──────

    #[test]
    fn sha1_empty_string() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha1_abc() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn sha1_long_message_spans_two_blocks() {
        // > 55 bytes forces a second 512-bit block (padding edge case).
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    // ── JSON round-trip / escaping ─────────────────────────────

    #[test]
    fn json_round_trips_special_chars() {
        let json = draft_to_json(
            "/tmp/re\"po\\x",
            "feat/new",
            "line1\nline2\ttab \"quote\"",
            "template",
            42,
        );
        let d = parse_draft_json(&json).expect("parse");
        assert_eq!(d.repo, "/tmp/re\"po\\x");
        assert_eq!(d.branch, "feat/new");
        assert_eq!(d.message, "line1\nline2\ttab \"quote\"");
        assert_eq!(d.mode, "template");
        assert_eq!(d.updated, 42);
    }

    #[test]
    fn parse_rejects_non_object() {
        assert!(parse_draft_json("not json").is_none());
        assert!(parse_draft_json("").is_none());
    }

    #[test]
    fn parse_lenient_defaults_for_optional_fields() {
        // Missing repo + mode + updated, but branch + message present.
        let d = parse_draft_json("{\"branch\":\"main\",\"message\":\"hi\"}").expect("parse");
        assert_eq!(d.repo, "");
        assert_eq!(d.mode, "plain");
        assert_eq!(d.updated, 0);
        assert_eq!(d.message, "hi");
    }

    #[test]
    fn empty_message_save_does_not_panic_on_path() {
        // A whitespace-only message routes to clear; verify that path-key
        // construction is stable for a representative repo + branch.
        let key = format!("{}\0{}", "/tmp/repo-a", "main");
        let name = sha1_hex(key.as_bytes());
        assert_eq!(name.len(), 40, "sha1 hex must be 40 chars");
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // NOTE: file-backed round-trip / branch-isolation / clear / empty-delete /
    // corrupt-JSON behaviour is covered in `tests/drafts_test.rs`. Those tests
    // mutate the process-global `KAGI_LOG_DIR` env var; keeping them in a
    // separate integration binary avoids racing against other lib unit tests
    // (e.g. the oplog env tests) that share the same process and env var.
}
