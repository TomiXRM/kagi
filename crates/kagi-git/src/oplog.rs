//! Operation log — T017
//!
//! Appends structured JSON Lines records to `$KAGI_LOG_DIR/operations.jsonl`
//! (or `$HOME/.kagi/operations.jsonl` if `KAGI_LOG_DIR` is not set).
//!
//! The file is created (and its parent directory auto-created) on first write.
//! Write failures are reported to stderr only — they never abort the application.
//!
//! The private serde wire schema preserves legacy receipt fields and defaults.
//! Append/retention own durability and identity; decoding never rewrites the log.
//!
//! # Public API
//!
//! - [`OpOutcome`] — operation result variant
//! - [`OpLogEntry`] — one log record
//! - [`append_oplog`] — write `entry` to the JSONL file

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ops::StateSummary, GitError};

mod codec;
mod reading;
pub mod recovery;
pub mod retention;
mod tail;

pub use recovery::RecoveryHandle;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// The result of a git operation.
#[derive(Debug, Clone)]
pub enum OpOutcome {
    /// Side effects or process termination cannot be fully established.
    Unknown {
        after: StateSummary,
        evidence: String,
    },
    /// Operation completed without error.
    Success {
        /// Repository state immediately after execution.
        after: StateSummary,
    },
    /// Operation was only PARTIALLY applied: repository side effects completed,
    /// but some requested changes failed. Recovery handles (e.g. backup blob
    /// SHAs or deleted ref OIDs) live in `after.dirty`.
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

/// A stable, machine-identifiable reason a recorded operation failed.
///
/// `OpOutcome::Failed { error }` persists English prose, so identifying *why*
/// something failed meant matching on that text — which breaks whenever Git,
/// libgit2 or a translation changes wording (#650). #500 moved recovery handles
/// out of prose into a structured field for the same reason.
///
/// **The serialized strings are the contract.** They are written to logs that
/// outlive this binary, so a variant's string must never change once shipped.
/// Renaming the Rust variant is fine; changing its `as_str` is not.
///
/// Unknown strings read back as [`FailureCode::Other`] rather than failing the
/// parse: a log written by a newer Kagi must still be readable by an older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    /// The repository workdir is owned by another uid and is not trusted.
    Untrusted,
    /// A deadline expired before the child was reaped: the operation may have
    /// happened. Never treat this as a retryable failure (ADR-0177).
    TerminationUnknown,
    /// Plan-time blockers refused the operation.
    Blocked,
    /// A preflight check failed before execution.
    Preflight,
    /// The stash entry could not be identified as the one that was planned.
    StashIdentityUnverified,
    /// Rebase refused because repository config would execute code.
    RebaseBlockedByRepoSettings,
    /// The path is not a repository, or does not exist.
    NotARepository,
    /// Anything without a dedicated code yet, including codes written by a
    /// newer Kagi than the one reading the log.
    Other,
}

impl FailureCode {
    /// The persisted string. **Never change one that has shipped.**
    pub fn as_str(self) -> &'static str {
        match self {
            FailureCode::Untrusted => "untrusted",
            FailureCode::TerminationUnknown => "termination-unknown",
            FailureCode::Blocked => "blocked",
            FailureCode::Preflight => "preflight",
            FailureCode::StashIdentityUnverified => "stash-identity-unverified",
            FailureCode::RebaseBlockedByRepoSettings => "rebase-blocked-by-repo-settings",
            FailureCode::NotARepository => "not-a-repository",
            FailureCode::Other => "other",
        }
    }

    /// Parse a persisted string. Anything unrecognised becomes
    /// [`FailureCode::Other`] so an older Kagi can read a newer log.
    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "untrusted" => FailureCode::Untrusted,
            "termination-unknown" => FailureCode::TerminationUnknown,
            "blocked" => FailureCode::Blocked,
            "preflight" => FailureCode::Preflight,
            "stash-identity-unverified" => FailureCode::StashIdentityUnverified,
            "rebase-blocked-by-repo-settings" => FailureCode::RebaseBlockedByRepoSettings,
            "not-a-repository" => FailureCode::NotARepository,
            _ => FailureCode::Other,
        }
    }
}

impl From<&crate::GitError> for FailureCode {
    fn from(error: &crate::GitError) -> Self {
        use crate::GitError;
        match error {
            GitError::Untrusted(_) => FailureCode::Untrusted,
            GitError::TerminationUnknown(_) => FailureCode::TerminationUnknown,
            GitError::Blocked(_) => FailureCode::Blocked,
            // A preflight failure wraps the underlying error; that it was
            // refused *before* execution is the useful distinction here.
            GitError::Preflight(_) => FailureCode::Preflight,
            GitError::StashIdentityUnverified(_) => FailureCode::StashIdentityUnverified,
            GitError::RebaseCannotStartWithRepoSettingsDisabled(_) => {
                FailureCode::RebaseBlockedByRepoSettings
            }
            GitError::NotARepository(_) | GitError::PathNotFound(_) => FailureCode::NotARepository,
            GitError::BareRepository(_) | GitError::Other(_) => FailureCode::Other,
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
    /// Machine-identifiable failure reason, when the outcome is a failure.
    ///
    /// Kept on the entry rather than inside `OpOutcome::Failed` so the 120-odd
    /// sites that construct a failure outcome stay untouched, and so an old log
    /// line without the field still parses (#650).
    pub failure_code: Option<FailureCode>,
    /// Absolute path to the worktree the operation ran in (`None` for old
    /// lines that predate the field).
    pub worktree: Option<String>,
    /// Repository state captured at plan time (before execution).
    pub before: StateSummary,
    /// Outcome of the operation.
    pub outcome: OpOutcome,
    /// Mandatory recovery roots, retained for the lifetime of this entry (#523).
    pub backup_refs: Vec<String>,
    /// Typed recovery handles — savepoint / stash OIDs and path→blob backups
    /// that used to be readable only out of the `after.dirty` sentence (#500).
    /// Additive: an entry written before this field reads back as empty.
    pub recovery: Vec<RecoveryHandle>,
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
            backup_refs: Vec::new(),
            recovery: Vec::new(),
            failure_code: None,
        }
    }

    /// Builder: set the initiating actor.
    /// Attach the machine-identifiable reason this operation failed.
    ///
    /// Callers that hold the typed `GitError` set this; the prose in
    /// `OpOutcome::Failed` stays as it is for people to read (#650).
    pub fn with_failure_code(mut self, code: FailureCode) -> Self {
        self.failure_code = Some(code);
        self
    }

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

/// Serialise an [`OpLogEntry`] as a single-line JSON object (no trailing newline).
pub fn entry_to_json(entry: &OpLogEntry) -> String {
    codec::to_json(entry)
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
///
/// Test binaries always carry `CARGO_MANIFEST_DIR`; they must explicitly set
/// `KAGI_LOG_DIR` so a failed fixture cannot write into a developer's home.
fn log_file_path() -> Result<Option<PathBuf>, GitError> {
    let log_dir = std::env::var_os("KAGI_LOG_DIR");
    let home = dirs_home();
    let test_runtime = std::env::var_os("CARGO_MANIFEST_DIR").is_some();
    log_file_path_from_env(log_dir.as_deref(), home.as_deref(), test_runtime)
}

fn log_file_path_from_env(
    log_dir: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
    test_runtime: bool,
) -> Result<Option<PathBuf>, GitError> {
    if let Some(dir) = log_dir.filter(|dir| !dir.is_empty()) {
        return Ok(Some(PathBuf::from(dir).join("operations.jsonl")));
    }
    if test_runtime {
        return Err(GitError::Other("tests must set KAGI_LOG_DIR".to_string()));
    }
    Ok(home
        .filter(|home| !home.as_os_str().is_empty())
        .map(|home| home.join(".kagi").join("operations.jsonl")))
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

#[cfg(test)]
fn parse_oplog_line(line: &str) -> Option<OpLogEntry> {
    codec::from_value(serde_json::from_str(line).ok()?)
}

/// Read the last `n` entries from the oplog file (newest last in file,
/// returned newest-first).
///
/// Uses the same path resolution as [`append_oplog`] (`$KAGI_LOG_DIR` first,
/// then `$HOME/.kagi/operations.jsonl`).
///
/// Only the tail of the file is read (#499): the cost of a read — including
/// the id/parent assignment inside a locked append — no longer grows with
/// total history. A legacy id-less line in that window still needs the whole
/// file, and non-UTF-8 bytes *older* than the window are deliberately no
/// longer read at all (see [`tail`]).
///
/// Lines that cannot be parsed are silently skipped.
/// Returns an empty `Vec` if the file does not exist or cannot be read.
pub fn read_oplog_tail(n: usize) -> Vec<OpLogEntry> {
    tail::read(n, &|_| true).entries
}

/// Read the last `n` oplog entries whose repository matches `repo`, newest
/// first — repo confinement for `kagi_oplog` / `kagi oplog --repo` (#421).
///
/// The oplog is a single global JSONL file; the MCP server and the CLI must
/// only ever surface the **bound** repository's history, never another repo's.
/// Paths are normalized (resolved to the repo workdir, then canonicalized) on
/// both sides before comparison, so a trailing slash, a `.` component, or a
/// symlink cannot defeat the filter.
///
/// Entries written from a *linked worktree* of the same repo are logged under
/// the worktree's own path (`Backend::run` records `self.path` for both `repo`
/// and `worktree`) and are therefore a **separate scope** — they are not
/// included here.
///
/// The global id/parent chain (ADR-0149) is reconstructed over the *whole* file
/// before filtering, so ids stay global; a filtered `parent` may point at an
/// entry that is not in the returned set (a future selective-undo must walk
/// backwards within the filtered set — noted for #334).
pub fn read_oplog_tail_for_repo(repo: &Path, n: usize) -> Vec<OpLogEntry> {
    let want = normalize_repo_path(repo);
    tail::read(n, &|entry| {
        normalize_repo_path(Path::new(&entry.repo)) == want
    })
    .entries
}

/// Normalize a repository path for oplog filtering: resolve to the repo workdir
/// (so `--repo <subdir>` or a `.git` path still matches entries logged under
/// the workdir root), then canonicalize to fold trailing slashes, `.`
/// components, and symlinks. Falls back to the raw path when the repo can't be
/// discovered (e.g. an entry whose repo was since deleted).
fn normalize_repo_path(path: &Path) -> PathBuf {
    let base = git2::Repository::discover(path)
        .ok()
        .and_then(|r| r.workdir().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf());
    std::fs::canonicalize(&base).unwrap_or(base)
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
    append_oplog_receipt(entry).map(|(path, _)| path)
}

/// Returns the exact assigned entry, never a tail read after the append.
pub fn append_oplog_receipt(entry: &OpLogEntry) -> Result<(PathBuf, OpLogEntry), GitError> {
    use std::io::Write;

    let path = log_file_path()?.ok_or_else(|| {
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

    let mut lock = retention::append_lock(&path)?;
    retention::validate_append_roots(entry)?;

    // ADR-0149: assign the sequence id/parent from the current tail so ids are
    // monotonic and each entry chains to the previous one. Placeholder id/parent
    // on `entry` (from `OpLogEntry::new`) are overwritten here.
    // Append and explicit retirement share the stable sidecar lock.
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

    entry.id = retention::reserve_id(&mut lock, entry.id)?;
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

    Ok((path, entry))
}

// ────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────

// Unit tests live in a child file to keep this file under the LOC ratchet
// (reader, retention, and receipt compatibility coverage).
#[cfg(test)]
#[path = "oplog_tests.rs"]
mod tests;

// ADR-0129 Phase 1 — oplog on-disk compatibility tests live in a child file
// (`oplog_adr0129_tests.rs`) to keep this file under the LOC ratchet.
#[cfg(test)]
#[path = "oplog_adr0129_tests.rs"]
mod adr0129_compat_tests;

#[cfg(test)]
mod failure_code_tests {
    use super::*;

    fn failed_entry() -> OpLogEntry {
        OpLogEntry::new(
            "fetch",
            "/tmp/repo",
            StateSummary {
                head: "branch: main".into(),
                dirty: "unchanged".into(),
            },
            OpOutcome::Failed {
                error: "boom".into(),
            },
        )
    }

    #[test]
    fn a_code_round_trips_through_the_persisted_line() {
        let entry = failed_entry().with_failure_code(FailureCode::TerminationUnknown);
        let line = entry_to_json(&entry);
        assert!(
            line.contains("\"failure_code\":\"termination-unknown\""),
            "the code must be a field, not prose: {line}"
        );
        let parsed = parse_oplog_line(&line).expect("the line we just wrote must parse");
        assert_eq!(parsed.failure_code, Some(FailureCode::TerminationUnknown));
    }

    #[test]
    fn an_entry_without_a_code_is_written_exactly_as_before() {
        let line = entry_to_json(&failed_entry());
        assert!(
            !line.contains("failure_code"),
            "an absent failure code must stay omitted, not become a null code: {line}"
        );
    }

    #[test]
    fn a_log_line_from_before_this_field_still_parses() {
        // The oplog outlives the binary that wrote it. A line written by any
        // earlier Kagi has no `failure_code` at all.
        let line = entry_to_json(&failed_entry());
        let parsed = parse_oplog_line(&line).expect("an old line must still parse");
        assert_eq!(
            parsed.failure_code, None,
            "absent means 'this Kagi did not record one', not 'no failure'"
        );
        assert!(matches!(parsed.outcome, OpOutcome::Failed { .. }));
    }

    #[test]
    fn a_code_from_a_newer_kagi_does_not_break_the_parse() {
        let line = entry_to_json(&failed_entry().with_failure_code(FailureCode::Untrusted))
            .replace("untrusted", "invented-by-a-later-version");
        let parsed = parse_oplog_line(&line)
            .expect("a newer log must stay readable by an older Kagi (#650)");
        assert_eq!(parsed.failure_code, Some(FailureCode::Other));
    }

    #[test]
    fn every_typed_error_maps_to_a_code() {
        use crate::GitError;
        for (error, expected) in [
            (GitError::Untrusted("x".into()), FailureCode::Untrusted),
            (
                GitError::TerminationUnknown(crate::Termination::stopped("x")),
                FailureCode::TerminationUnknown,
            ),
            (
                GitError::RebaseCannotStartWithRepoSettingsDisabled("x".into()),
                FailureCode::RebaseBlockedByRepoSettings,
            ),
            (
                GitError::StashIdentityUnverified("x".into()),
                FailureCode::StashIdentityUnverified,
            ),
            (
                GitError::NotARepository("x".into()),
                FailureCode::NotARepository,
            ),
            (GitError::Other("x".into()), FailureCode::Other),
        ] {
            assert_eq!(FailureCode::from(&error), expected, "for {error:?}");
        }
    }

    #[test]
    fn shipped_code_strings_never_change() {
        // These strings are written into logs on users' disks. Renaming a Rust
        // variant is fine; changing one of these is not, and this test is the
        // thing that says so out loud.
        for (code, expected) in [
            (FailureCode::Untrusted, "untrusted"),
            (FailureCode::TerminationUnknown, "termination-unknown"),
            (FailureCode::Blocked, "blocked"),
            (FailureCode::Preflight, "preflight"),
            (
                FailureCode::StashIdentityUnverified,
                "stash-identity-unverified",
            ),
            (
                FailureCode::RebaseBlockedByRepoSettings,
                "rebase-blocked-by-repo-settings",
            ),
            (FailureCode::NotARepository, "not-a-repository"),
            (FailureCode::Other, "other"),
        ] {
            assert_eq!(code.as_str(), expected);
            assert_eq!(FailureCode::from_str_lossy(expected), code);
        }
    }

    #[test]
    fn a_recorded_failure_carries_its_code_end_to_end() {
        // The code is only worth having if it reaches the persisted line from a
        // real recording, not just from a hand-built entry.
        let entry = failed_entry().with_failure_code(FailureCode::from(
            &crate::GitError::Untrusted("/repo".into()),
        ));
        let line = entry_to_json(&entry);
        let parsed = parse_oplog_line(&line).expect("parses");
        assert_eq!(parsed.failure_code, Some(FailureCode::Untrusted));
        // The prose is untouched: people still read the sentence.
        match parsed.outcome {
            OpOutcome::Failed { error } => assert_eq!(error, "boom"),
            other => panic!("expected a failure outcome, got {other:?}"),
        }
    }
}
