//! File History data layer (ADR-0089, ADR-0108).
//!
//! **Name note (ADR-0108):** this module was renamed from `history.rs` to
//! `file_history.rs` to resolve a filename collision with
//! `kagi_domain::history`, which is the *operation* history (undo/redo stack).
//! This module is the *file* history — the per-path commit log. The two
//! modules describe different concepts and previously shared a filename,
//! making it unclear which `history::Foo` a caller meant.
//!
//! Collects the per-commit change history of a single file via the `git` CLI
//! (`cli::run_git`), with optional rename-following (`git log --follow`) and an
//! optional synthetic "WIP" entry for uncommitted working-tree changes.
//!
//! This module is pure data + CLI orchestration: it does **not** depend on
//! `git2` and it does **not** produce diffs.  The UI reuses the existing
//! `Backend` diff methods (`commit_file_diff`, `unstaged_file_diff`,
//! `staged_file_diff`) to render each entry's patch.
//!
//! # Robust parsing
//!
//! A single `git log` invocation is used with explicit record / field
//! separators so the free-form commit body cannot corrupt parsing:
//!
//! - Records are separated by `\x1e` (ASCII record separator).
//! - The leading metadata fields are separated by `\x1f` (ASCII unit
//!   separator).
//! - `--raw` and `--numstat` are requested together.  `--raw` supplies the
//!   change-type letter (`A`/`M`/`D`/`R###`/`C###`) and clean tab-separated
//!   paths, while `--numstat` supplies the insertion/deletion counts (`-`
//!   meaning binary).  These two flags interleave per commit; `--name-status`
//!   is intentionally avoided because it suppresses `--numstat` output when
//!   both are present.
//! - `-c core.quotePath=false` keeps non-ASCII paths as raw UTF-8 instead of
//!   octal-escaped, C-quoted strings.

use std::path::{Path, PathBuf};

use super::cli::run_git;
use super::GitError;

// ────────────────────────────────────────────────────────────
// Models (pure data, no git2)
// ────────────────────────────────────────────────────────────

/// Parameters for a file-history query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryRequest {
    /// Working-tree root of the repository.
    pub repo_dir: PathBuf,
    /// Target file, **repo-relative**.
    pub file_path: PathBuf,
    /// Follow the file across renames (`git log --follow`).
    pub follow_renames: bool,
    /// Prepend a synthetic WIP entry when there are uncommitted changes.
    pub include_wip: bool,
    /// Maximum number of commit entries (`git log -n`); `0` means unlimited.
    pub limit: usize,
}

/// The collected history of a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistory {
    /// The file's current (most recent) path, repo-relative.
    pub current_path: PathBuf,
    /// Entries, WIP first (if any), then commits newest-first.
    pub entries: Vec<FileHistoryEntry>,
}

/// A single row in the history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryEntry {
    /// Whether this is the synthetic WIP row or a real commit.
    pub kind: FileHistoryEntryKind,
    /// Commit metadata; `None` for the WIP entry.
    pub commit: Option<CommitSummary>,
    /// The change this entry made to the file.
    pub change: FileChangeSummary,
}

/// Discriminates a real commit row from the synthetic working-tree row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHistoryEntryKind {
    /// Uncommitted working-tree / index / untracked change.
    Wip,
    /// A committed change.
    Commit,
}

/// Commit metadata for a history row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub full_hash: String,
    pub short_hash: String,
    pub subject: String,
    pub body: Option<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub committer_name: String,
    pub committer_date: String,
}

/// How the file changed in a given entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChangeSummary {
    pub change_type: FileChangeType,
    /// Previous path for renames/copies; `None` otherwise.
    pub path_before: Option<PathBuf>,
    /// The file's path at this entry.
    pub path_after: PathBuf,
    /// Added lines; `None` when unknown (binary).
    pub insertions: Option<u32>,
    /// Removed lines; `None` when unknown (binary).
    pub deletions: Option<u32>,
    /// Whether git reported this as a binary change.
    pub is_binary: bool,
}

/// The kind of change recorded for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unknown,
}

// ────────────────────────────────────────────────────────────
// Record / field separators (must match the --format string).
// ────────────────────────────────────────────────────────────

const RS: char = '\u{1e}'; // record separator (between commits)
const FS: char = '\u{1f}'; // field separator (between metadata fields)

/// `--format` placing 9 separator-delimited metadata fields, then a trailing
/// FS so the body field is unambiguously bounded before the raw/numstat block.
const LOG_FORMAT: &str = concat!(
    "\u{1e}", // record start
    "%H", "\u{1f}", "%h", "\u{1f}", "%s", "\u{1f}", "%an", "\u{1f}", "%ae", "\u{1f}", "%aI",
    "\u{1f}", "%cn", "\u{1f}", "%cI", "\u{1f}", "%b", "\u{1f}",
);

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Collect the change history of a single file.
///
/// Runs one `git log` to enumerate commits that touched the file (optionally
/// following renames), then — if requested — a `git status` to synthesise a
/// leading WIP entry.  Non-zero git exit status is surfaced as
/// [`GitError::Other`] including stderr.  An empty log is **not** an error: it
/// yields an empty (or WIP-only) history.
pub fn file_history(req: &FileHistoryRequest) -> Result<FileHistory, GitError> {
    let path_str = req.file_path.to_string_lossy();
    let path_arg: &str = &path_str;

    // ── git log ────────────────────────────────────────────────
    let format_arg = format!("--format={}", LOG_FORMAT);
    let limit_arg;
    let mut args: Vec<&str> = vec![
        "-c",
        "core.quotePath=false",
        "log",
        "--find-renames",
        "--date=iso-strict",
        "--raw",
        "--numstat",
        &format_arg,
    ];
    if req.follow_renames {
        args.push("--follow");
    }
    if req.limit > 0 {
        limit_arg = format!("-n{}", req.limit);
        args.push(&limit_arg);
    }
    args.push("--");
    args.push(path_arg);

    let out = run_git(&req.repo_dir, &args)?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "git log for file history failed (status {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let mut entries = parse_log(&out.stdout, &req.file_path);

    // current_path: the path at the newest commit, else the requested path.
    let current_path = entries
        .first()
        .map(|e| e.change.path_after.clone())
        .unwrap_or_else(|| req.file_path.clone());

    // ── WIP entry ──────────────────────────────────────────────
    if req.include_wip {
        if let Some(wip) = wip_entry(req, &current_path)? {
            entries.insert(0, wip);
        }
    }

    Ok(FileHistory {
        current_path,
        entries,
    })
}

// ────────────────────────────────────────────────────────────
// Parsing
// ────────────────────────────────────────────────────────────

/// Parse the `git log --raw --numstat` output into commit entries
/// (newest-first, matching git log order).
fn parse_log(stdout: &str, requested: &Path) -> Vec<FileHistoryEntry> {
    let mut entries = Vec::new();

    // Split into per-commit records on the record separator.  The first split
    // chunk before the first RS is empty (or whitespace) and is skipped.
    for record in stdout.split(RS) {
        if record.trim().is_empty() {
            continue;
        }
        if let Some(entry) = parse_record(record, requested) {
            entries.push(entry);
        }
    }

    entries
}

/// Parse one commit record: the FS-delimited metadata fields followed by the
/// `--raw` and `--numstat` lines for the file.
fn parse_record(record: &str, requested: &Path) -> Option<FileHistoryEntry> {
    // The format emits exactly 9 metadata fields, each terminated by FS:
    // H, h, s, an, ae, aI, cn, cI, b — note the body (b) is last and may
    // contain newlines but never FS/RS, so splitting on FS is safe.
    let mut parts = record.splitn(10, FS);
    let full_hash = parts.next()?.trim_start_matches('\n').to_string();
    let short_hash = parts.next()?.to_string();
    let subject = parts.next()?.to_string();
    let author_name = parts.next()?.to_string();
    let author_email = parts.next()?.to_string();
    let author_date = parts.next()?.to_string();
    let committer_name = parts.next()?.to_string();
    let committer_date = parts.next()?.to_string();
    let body_raw = parts.next().unwrap_or("");
    // Remainder (after the 9th FS) holds the raw + numstat lines.
    let trailer = parts.next().unwrap_or("");

    let body = {
        let trimmed = body_raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let commit = CommitSummary {
        full_hash,
        short_hash,
        subject,
        body,
        author_name,
        author_email,
        author_date,
        committer_name,
        committer_date,
    };

    let change = parse_change(trailer, requested);

    Some(FileHistoryEntry {
        kind: FileHistoryEntryKind::Commit,
        commit: Some(commit),
        change,
    })
}

/// Parse the raw + numstat trailing block of a single commit record into a
/// [`FileChangeSummary`].
///
/// `--raw` line:    `:<mode> <mode> <sha> <sha> <STATUS>\t<path>[\t<path2>]`
/// `--numstat` line: `<ins>\t<del>\t<path>`  (`-` for binary)
fn parse_change(trailer: &str, requested: &Path) -> FileChangeSummary {
    let mut change_type = FileChangeType::Unknown;
    let mut path_before: Option<PathBuf> = None;
    let mut path_after: PathBuf = requested.to_path_buf();
    let mut insertions: Option<u32> = None;
    let mut deletions: Option<u32> = None;
    let mut is_binary = false;
    let mut saw_raw = false;
    let mut saw_numstat = false;

    for line in trailer.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(':') {
            // --raw line.  Status + paths come after the four blob fields.
            if saw_raw {
                continue; // only the first file entry of interest
            }
            if let Some((status, before, after)) = parse_raw_line(rest) {
                change_type = status;
                path_before = before;
                path_after = after;
                saw_raw = true;
            }
        } else {
            // --numstat line: ins<TAB>del<TAB>path
            if saw_numstat {
                continue;
            }
            let mut cols = line.splitn(3, '\t');
            let ins = cols.next().unwrap_or("");
            let del = cols.next().unwrap_or("");
            // Third column (path) is ignored; --raw provides clean paths.
            if cols.next().is_none() {
                continue; // malformed
            }
            if ins == "-" || del == "-" {
                is_binary = true;
                insertions = None;
                deletions = None;
            } else {
                insertions = ins.parse::<u32>().ok();
                deletions = del.parse::<u32>().ok();
            }
            saw_numstat = true;
        }
    }

    FileChangeSummary {
        change_type,
        path_before,
        path_after,
        insertions,
        deletions,
        is_binary,
    }
}

/// Parse the portion of a `--raw` line after the leading `:`.
///
/// Layout: `<mode> <mode> <sha> <sha> <STATUS>\t<path>[\t<path2>]`.  The five
/// metadata tokens are space-separated; paths are tab-separated.
fn parse_raw_line(rest: &str) -> Option<(FileChangeType, Option<PathBuf>, PathBuf)> {
    // Split off the tab-delimited path section from the space-delimited meta.
    let (meta, paths) = rest.split_once('\t')?;
    let status = meta.split_whitespace().last()?;
    let change_type = status_letter(status);

    let mut path_cols = paths.split('\t');
    let first = path_cols.next()?;
    let second = path_cols.next();

    match (change_type, second) {
        (FileChangeType::Renamed | FileChangeType::Copied, Some(new_path)) => Some((
            change_type,
            Some(PathBuf::from(first)),
            PathBuf::from(new_path),
        )),
        _ => Some((change_type, None, PathBuf::from(first))),
    }
}

/// Map a `--raw` status token (e.g. `M`, `R100`, `C75`) to a [`FileChangeType`].
fn status_letter(status: &str) -> FileChangeType {
    match status.chars().next() {
        Some('A') => FileChangeType::Added,
        Some('M') => FileChangeType::Modified,
        Some('D') => FileChangeType::Deleted,
        Some('R') => FileChangeType::Renamed,
        Some('C') => FileChangeType::Copied,
        _ => FileChangeType::Unknown,
    }
}

// ────────────────────────────────────────────────────────────
// WIP detection
// ────────────────────────────────────────────────────────────

/// Build the synthetic WIP entry from `git status --porcelain=v1`, or `None`
/// when the file has no uncommitted change.
fn wip_entry(
    req: &FileHistoryRequest,
    current_path: &Path,
) -> Result<Option<FileHistoryEntry>, GitError> {
    let path_str = req.file_path.to_string_lossy();
    let args = [
        "-c",
        "core.quotePath=false",
        "status",
        "--porcelain=v1",
        "--",
        &path_str,
    ];
    let out = run_git(&req.repo_dir, &args)?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "git status for file history failed (status {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    // Take the first non-empty status line for the file.  Porcelain v1 lines
    // are `XY <path>` (or `XY <old> -> <new>` for renames).
    let line = match out.stdout.lines().find(|l| !l.trim().is_empty()) {
        Some(l) => l,
        None => return Ok(None),
    };
    if line.len() < 3 {
        return Ok(None);
    }
    let code = &line[..2];
    let change_type = wip_change_type(code);

    let change = FileChangeSummary {
        change_type,
        path_before: None,
        path_after: current_path.to_path_buf(),
        insertions: None,
        deletions: None,
        is_binary: false,
    };

    Ok(Some(FileHistoryEntry {
        kind: FileHistoryEntryKind::Wip,
        commit: None,
        change,
    }))
}

/// Map a porcelain-v1 `XY` status code to a [`FileChangeType`].
fn wip_change_type(code: &str) -> FileChangeType {
    if code == "??" {
        return FileChangeType::Added; // untracked
    }
    if code.contains('R') {
        return FileChangeType::Renamed;
    }
    if code.contains('D') {
        return FileChangeType::Deleted;
    }
    if code.contains('A') {
        return FileChangeType::Added;
    }
    FileChangeType::Modified
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure parsing — no git invocation)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(meta: &str, trailer: &str) -> String {
        // Build a record body (without the leading RS, which split() removes).
        format!("{meta}\n\n{trailer}")
    }

    #[test]
    fn parses_added_modified_in_order() {
        // Two records, newest first, as git log emits them.
        let meta_new = "h2\u{1f}h2s\u{1f}modify\u{1f}Ann\u{1f}a@x\u{1f}2026-01-02T00:00:00+00:00\u{1f}Bob\u{1f}2026-01-02T00:00:00+00:00\u{1f}\u{1f}";
        let meta_old = "h1\u{1f}h1s\u{1f}add\u{1f}Ann\u{1f}a@x\u{1f}2026-01-01T00:00:00+00:00\u{1f}Bob\u{1f}2026-01-01T00:00:00+00:00\u{1f}\u{1f}";
        let stdout = format!(
            "{RS}{}{RS}{}",
            rec(
                meta_new,
                ":100644 100644 aaa bbb M\tfoo.txt\n12\t4\tfoo.txt"
            ),
            rec(meta_old, ":000000 100644 000 aaa A\tfoo.txt\n3\t0\tfoo.txt"),
        );

        let entries = parse_log(&stdout, Path::new("foo.txt"));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].change.change_type, FileChangeType::Modified);
        assert_eq!(entries[0].change.insertions, Some(12));
        assert_eq!(entries[0].change.deletions, Some(4));
        assert_eq!(entries[0].commit.as_ref().unwrap().subject, "modify");
        assert_eq!(entries[1].change.change_type, FileChangeType::Added);
        assert_eq!(entries[1].change.insertions, Some(3));
    }

    #[test]
    fn parses_rename_paths() {
        let meta = "h\u{1f}hs\u{1f}rename\u{1f}A\u{1f}a@x\u{1f}d\u{1f}B\u{1f}d\u{1f}\u{1f}";
        let stdout = format!(
            "{RS}{}",
            rec(
                meta,
                ":100644 100644 aaa aaa R100\told.txt\tnew.txt\n0\t0\told.txt => new.txt"
            )
        );
        let entries = parse_log(&stdout, Path::new("new.txt"));
        assert_eq!(entries[0].change.change_type, FileChangeType::Renamed);
        assert_eq!(
            entries[0].change.path_before,
            Some(PathBuf::from("old.txt"))
        );
        assert_eq!(entries[0].change.path_after, PathBuf::from("new.txt"));
    }

    #[test]
    fn parses_binary_change() {
        let meta = "h\u{1f}hs\u{1f}bin\u{1f}A\u{1f}a@x\u{1f}d\u{1f}B\u{1f}d\u{1f}\u{1f}";
        let stdout = format!(
            "{RS}{}",
            rec(meta, ":000000 100644 000 ccc A\tbin.dat\n-\t-\tbin.dat")
        );
        let entries = parse_log(&stdout, Path::new("bin.dat"));
        assert!(entries[0].change.is_binary);
        assert_eq!(entries[0].change.insertions, None);
        assert_eq!(entries[0].change.deletions, None);
    }

    #[test]
    fn body_with_newlines_does_not_corrupt() {
        let meta =
            "h\u{1f}hs\u{1f}subj\u{1f}A\u{1f}a@x\u{1f}d\u{1f}B\u{1f}d\u{1f}line1\nline2\n\u{1f}";
        let stdout = format!(
            "{RS}{}",
            rec(meta, ":100644 100644 aaa bbb M\tf.txt\n1\t1\tf.txt")
        );
        let entries = parse_log(&stdout, Path::new("f.txt"));
        assert_eq!(
            entries[0].commit.as_ref().unwrap().body,
            Some("line1\nline2".to_string())
        );
        assert_eq!(entries[0].change.change_type, FileChangeType::Modified);
    }

    #[test]
    fn wip_code_mapping() {
        assert_eq!(wip_change_type("??"), FileChangeType::Added);
        assert_eq!(wip_change_type(" M"), FileChangeType::Modified);
        assert_eq!(wip_change_type("M "), FileChangeType::Modified);
        assert_eq!(wip_change_type("MD"), FileChangeType::Deleted);
        assert_eq!(wip_change_type("R "), FileChangeType::Renamed);
        assert_eq!(wip_change_type("A "), FileChangeType::Added);
    }
}
