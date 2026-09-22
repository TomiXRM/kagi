//! Shared commit-message / Issue draft autosave — ADR-0042.
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
//! - Outer [`Draft`] record: JSON encoded/decoded with serde, retaining the
//!   existing field names and defaults for missing optional fields.
//! - Issue draft `message`: a `serde_json` string tuple `[title, body]` inside
//!   that outer record. Commit-message draft payloads remain plain strings.
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
//! - [`queue_issue_draft`] / [`flush_issue_draft_if_version`] — keyed Issue autosave
//! - [`flush_issue_drafts`] — best-effort flush of every pending Issue draft
//! - [`load_issue_draft`] — read the latest pending or saved Issue draft
//! - [`issue_draft_version`] / [`clear_issue_draft_if_version`] — completion guards

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::GitError;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// A decoded commit-message draft for a single repository + branch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Draft {
    /// Absolute path to the repository working tree the draft belongs to.
    #[serde(default)]
    pub repo: String,
    /// Short branch name the draft belongs to, e.g. `"main"`.
    pub branch: String,
    /// The work-in-progress commit message (template drafts store the
    /// already-expanded plain text — see ADR-0042).
    pub message: String,
    /// Editor mode the message was authored in: `"plain"` or `"template"`.
    #[serde(default = "default_draft_mode")]
    pub mode: String,
    /// Unix epoch seconds at the time the draft was last written.
    #[serde(default)]
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
        GitError::Other(
            "draft: could not determine drafts dir (no HOME or KAGI_LOG_DIR)".to_string(),
        )
    })?;

    save_draft_at(&path, repo_path, branch, message, mode)
}

// The queue fixes the storage path before its debounce/background flush. Both
// APIs share the same serialization and atomic replacement implementation.
fn save_draft_at(
    path: &Path,
    repo_path: &Path,
    branch: &str,
    message: &str,
    mode: &str,
) -> Result<(), GitError> {
    if message.trim().is_empty() {
        return clear_draft_at(path);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!(
                "draft: mkdir failed for {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    let repo_str = repo_path.to_string_lossy();
    let updated = now_unix();
    let json = draft_to_json(&repo_str, branch, message, mode, updated)
        .map_err(|e| GitError::Other(format!("draft: serialize failed: {e}")))?;

    // Both commit and Issue drafts use this atomic replacement boundary.
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| GitError::Other("draft: missing directory".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| GitError::Other(format!("draft: temporary file: {e}")))?;
    temporary
        .write_all(json.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|e| GitError::Other(format!("draft: write failed: {e}")))?;
    temporary
        .persist(path)
        .map_err(|e| GitError::Other(format!("draft: replace {}: {e}", path.display())))?;

    Ok(())
}

/// Load the draft for `branch` in the repository at `repo_path`.
///
/// Returns `None` when no draft exists, the file cannot be read, or the JSON is
/// corrupt — a broken draft must never prevent the user from committing.
pub fn load_draft(repo_path: &Path, branch: &str) -> Option<Draft> {
    let path = draft_file_path(repo_path, branch)?;
    load_draft_at(&path)
}

fn load_draft_at(path: &Path) -> Option<Draft> {
    let content = std::fs::read_to_string(path).ok()?;
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
        GitError::Other(
            "draft: could not determine drafts dir (no HOME or KAGI_LOG_DIR)".to_string(),
        )
    })?;

    clear_draft_at(&path)
}

fn clear_draft_at(path: &Path) -> Result<(), GitError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GitError::Other(format!(
            "draft: delete failed for {}: {}",
            path.display(),
            e
        ))),
    }
}

/// A colon cannot occur in a Git branch name, so Issue keys never collide with
/// commit-message drafts. Storage is captured when queued, not when flushed.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IssueDraftKey {
    path: Option<PathBuf>,
    repo: PathBuf,
    branch: String,
}

impl IssueDraftKey {
    fn new(repo: &Path, number: Option<u64>) -> Self {
        let branch = match number {
            Some(number) => format!(":issue:{number}"),
            None => ":issue:new".to_owned(),
        };
        Self {
            path: draft_file_path(repo, &branch),
            repo: repo.to_path_buf(),
            branch,
        }
    }
}

#[derive(Default)]
struct IssueDraftQueue {
    pending: BTreeMap<IssueDraftKey, (String, String)>,
    // Retain versions after flush and clear for process-lifetime completion
    // guards, including when the tab that dispatched a write has closed.
    versions: BTreeMap<IssueDraftKey, u64>,
    last_version: u64,
}

impl IssueDraftQueue {
    fn advance(&mut self, key: &IssueDraftKey) -> u64 {
        self.last_version = self
            .last_version
            .checked_add(1)
            .expect("Issue draft version exhausted");
        self.versions.insert(key.clone(), self.last_version);
        self.last_version
    }
}

fn issue_drafts() -> &'static Mutex<IssueDraftQueue> {
    static QUEUE: OnceLock<Mutex<IssueDraftQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(IssueDraftQueue::default()))
}

/// Replace the latest pending Issue draft in memory, without filesystem I/O.
///
/// `None` is the new-Issue Composer; `Some(number)` is that Issue's Reply.
/// Queue two empty strings to clear. The caller schedules a background flush
/// through [`flush_issue_draft_if_version`] after its debounce; application
/// shutdown calls [`flush_issue_drafts`] for anything still pending.
/// Returns the new token to capture when dispatching an Issue write.
pub fn queue_issue_draft(repo: &Path, number: Option<u64>, title: &str, body: &str) -> u64 {
    let key = IssueDraftKey::new(repo, number);
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let version = queue.advance(&key);
    let value = if title.trim().is_empty() && body.trim().is_empty() {
        (String::new(), String::new())
    } else {
        (title.to_owned(), body.to_owned())
    };
    queue.pending.insert(key, value);
    version
}

/// Read (or allocate) the process-lifetime version of this storage key, without
/// filesystem I/O. Capture before loading and check again before applying it.
pub fn issue_draft_version(repo: &Path, number: Option<u64>) -> u64 {
    let key = IssueDraftKey::new(repo, number);
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match queue.versions.get(&key) {
        Some(version) => *version,
        None => queue.advance(&key),
    }
}

/// Consume the posted version only if it is still current, queuing a clear.
/// No tab needs to remain alive and no disk I/O occurs here. Globally unique
/// tokens locate their original storage key even if KAGI_LOG_DIR has changed;
/// repo and Issue identity must still match. A repeated completion is a no-op.
pub fn clear_issue_draft_if_version(repo: &Path, number: Option<u64>, version: u64) -> bool {
    let requested = IssueDraftKey::new(repo, number);
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = queue.versions.iter().find_map(|(key, current)| {
        (*current == version && key.repo == requested.repo && key.branch == requested.branch)
            .then(|| key.clone())
    });
    let Some(key) = key else {
        return false;
    };
    queue.advance(&key);
    queue.pending.insert(key, (String::new(), String::new()));
    true
}

/// Persist one pending Issue draft only while `version` still identifies its
/// `(repo, number, storage path)` key.
///
/// Returns `Ok(false)` when the timer was superseded or another flush already
/// persisted the value. A failure leaves this entry pending for retry and can
/// therefore be reported by its own editor without borrowing another draft's
/// aggregate [`flush_issue_drafts`] error.
pub fn flush_issue_draft_if_version(
    repo: &Path,
    number: Option<u64>,
    version: u64,
) -> Result<bool, GitError> {
    let requested = IssueDraftKey::new(repo, number);
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = queue.versions.iter().find_map(|(key, current)| {
        (*current == version && key.repo == requested.repo && key.branch == requested.branch)
            .then(|| key.clone())
    });
    let Some(key) = key else {
        return Ok(false);
    };
    let Some((title, body)) = queue.pending.get(&key) else {
        return Ok(false);
    };
    persist_issue_draft(&key, title, body)?;
    queue.pending.remove(&key);
    Ok(true)
}

fn persist_issue_draft(key: &IssueDraftKey, title: &str, body: &str) -> Result<(), GitError> {
    let Some(path) = key.path.as_deref() else {
        return Err(GitError::Other(
            "draft: could not determine drafts dir (no HOME or KAGI_LOG_DIR)".into(),
        ));
    };
    let message = if title.is_empty() && body.is_empty() {
        String::new()
    } else {
        // A string tuple serializes without a fallible custom serializer. The
        // outer ADR-0042 record stays unchanged.
        serde_json::json!([title, body]).to_string()
    };
    save_draft_at(path, &key.repo, &key.branch, &message, "issue-composer")
}

/// Persist all pending Issue drafts using the existing atomic draft boundary.
///
/// Keep the lock during writes: an older flush cannot run after a newer clear,
/// resurrecting a submitted draft. Failed entries remain queued for retry;
/// independent entries are still attempted and the first failure is returned.
pub fn flush_issue_drafts() -> Result<(), GitError> {
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut first_error = None;
    queue.pending.retain(|key, (title, body)| {
        let result = persist_issue_draft(key, title, body);
        match result {
            Ok(()) => false,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                true
            }
        }
    });
    first_error.map_or(Ok(()), Err)
}

/// Load the most recent Issue draft, including an update not yet flushed.
/// A pending clear takes precedence over an older file. Missing/corrupt or
/// unrelated-mode files follow the existing lenient load contract.
pub fn load_issue_draft(repo: &Path, number: Option<u64>) -> Option<(String, String)> {
    let key = IssueDraftKey::new(repo, number);
    let queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((title, body)) = queue.pending.get(&key) {
        return if title.is_empty() && body.is_empty() {
            None
        } else {
            Some((title.clone(), body.clone()))
        };
    }
    let draft = load_draft_at(key.path.as_deref()?)?;
    if draft.mode != "issue-composer" {
        return None;
    }
    let (title, body): (String, String) = serde_json::from_str(&draft.message).ok()?;
    if title.trim().is_empty() && body.trim().is_empty() {
        None
    } else {
        Some((title, body))
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
    let dir = drafts_dir()?;
    Some(draft_file_path_in_dir(repo_path, branch, &dir))
}

/// Full draft-file path under an already-resolved drafts directory.
///
/// Keeping the environment lookup in [`draft_file_path`] makes the key
/// construction independently testable without reading process-global state.
fn draft_file_path_in_dir(repo_path: &Path, branch: &str, dir: &Path) -> PathBuf {
    let key = format!("{}\0{}", repo_path.to_string_lossy(), branch);
    let name = format!("{}.json", sha1_hex(key.as_bytes()));
    dir.join(name)
}

/// Current wall-clock time in Unix epoch seconds (0 on a clock error).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// JSON record serialization
// ────────────────────────────────────────────────────────────

/// Borrowed write view: autosave must not copy the draft body to serialize it.
#[derive(serde::Serialize)]
struct DraftRecord<'a> {
    repo: &'a str,
    branch: &'a str,
    message: &'a str,
    mode: &'a str,
    updated: u64,
}

fn default_draft_mode() -> String {
    "plain".to_owned()
}

/// Render the existing ADR-0042 object schema without copying its string fields.
fn draft_to_json(
    repo: &str,
    branch: &str,
    message: &str,
    mode: &str,
    updated: u64,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&DraftRecord {
        repo,
        branch,
        message,
        mode,
        updated,
    })
}

/// Parse a draft JSON object produced by [`draft_to_json`].
///
/// Returns `None` when the required `branch` / `message` fields are missing or
/// the input is not a JSON object — corrupt drafts are treated as "no draft".
/// `repo` defaults to empty and `mode` to `"plain"` if absent (lenient read).
fn parse_draft_json(content: &str) -> Option<Draft> {
    // Serde also accepts positional arrays for structs; drafts require an object.
    if !content.trim_start().starts_with('{') {
        return None;
    }
    serde_json::from_str(content).ok()
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
pub(crate) fn sha1_hex(data: &[u8]) -> String {
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

    /// #476: a commit panel pointed at a linked worktree saves its draft under
    /// that worktree's path. The key is `sha1(repo_path \0 branch)`, so the
    /// same branch name in two repositories can never share a draft file —
    /// which is what keeps a worktree panel's message out of the open tab's.
    #[test]
    fn draft_key_includes_the_repo_path() {
        let temp = tempfile::tempdir().expect("temp drafts dir");
        let a = draft_file_path_in_dir(Path::new("/repo"), "main", temp.path());
        let b = draft_file_path_in_dir(Path::new("/repo/../wt"), "main", temp.path());
        assert_ne!(a, b, "same branch, different repos must not share a draft");
        // …and the branch still separates two drafts within one repository.
        let c = draft_file_path_in_dir(Path::new("/repo"), "feat", temp.path());
        assert_ne!(a, c);
        // Same inputs → same file (the load side must find what save wrote).
        assert_eq!(
            a,
            draft_file_path_in_dir(Path::new("/repo"), "main", temp.path())
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
        )
        .expect("serialize");
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
        assert!(parse_draft_json(r#"["/repo","main","message","plain",42]"#).is_none());
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

    // NOTE: file-backed round-trip / branch-isolation / clear / empty-delete /
    // corrupt-JSON behaviour is covered in `tests/drafts_test.rs`. Those tests
    // mutate the process-global `KAGI_LOG_DIR` env var; keeping them in a
    // separate integration binary avoids racing against other lib unit tests
    // (e.g. the oplog env tests) that share the same process and env var.
}
