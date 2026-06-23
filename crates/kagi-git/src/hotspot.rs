//! Ecosystem / hot-spot data layer (ADR-0119).
//!
//! Mines the whole-repo change history with the `git` CLI (`cli::run_git`,
//! like [`crate::file_history`]) and scans the working tree for current line
//! counts, producing the pure [`RawEcosystem`] that `kagi_domain::hotspot`
//! ranks. **No `git2`** — read-only, no diffs.
//!
//! # Parsing
//!
//! One `git log --numstat` invocation with a record separator per commit:
//!
//! - `--format=%x1e%at` prefixes each commit with `\x1e` (record separator)
//!   followed by the author time (epoch secs).
//! - `--numstat` then lists `<ins>\t<del>\t<path>` rows (`-` for binary).
//! - `--no-renames` keeps every path plain (a rename reads as delete + add,
//!   which is fine for churn counting) and avoids the `{old => new}` numstat
//!   form.
//! - Merge commits emit no numstat rows by default → they contribute no file
//!   churn, which is the desired behaviour.
//! - `-c core.quotePath=false` keeps non-ASCII paths as raw UTF-8.

use std::path::{Path, PathBuf};

use super::cli::run_git;
use super::GitError;
use kagi_domain::hotspot::{CommitChanges, FileChange, RawEcosystem};

/// Record separator emitted by `--format=%x1e…` before each commit.
const RS: char = '\u{1e}';

/// Parameters for a whole-repo ecosystem scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcosystemRequest {
    /// Working-tree root of the repository.
    pub repo_dir: PathBuf,
    /// Maximum number of commits to mine (`git log -n`); `0` means unlimited.
    pub limit: usize,
}

/// Mine the repository into a [`RawEcosystem`]: every commit's changed files
/// (with author time) plus the current line count of each touched file.
///
/// Non-zero git exit status is surfaced as [`GitError::Other`]. An empty log is
/// not an error — it yields an empty ecosystem.
pub fn repo_ecosystem(req: &EcosystemRequest) -> Result<RawEcosystem, GitError> {
    let limit_arg;
    let mut args: Vec<&str> = vec![
        "-c",
        "core.quotePath=false",
        "log",
        "--no-renames",
        "--numstat",
        "--format=%x1e%at",
    ];
    if req.limit > 0 {
        limit_arg = format!("-n{}", req.limit);
        args.push(&limit_arg);
    }

    let out = run_git(&req.repo_dir, &args)?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "git log for ecosystem failed (status {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let commits = parse_numstat_log(&out.stdout);
    let loc = scan_loc(&req.repo_dir, &commits);
    Ok(RawEcosystem { commits, loc })
}

/// Parse `git log --numstat --format=%x1e%at` output into per-commit changes.
fn parse_numstat_log(stdout: &str) -> Vec<CommitChanges> {
    let mut commits = Vec::new();
    for record in stdout.split(RS) {
        if record.trim().is_empty() {
            continue;
        }
        let mut lines = record.lines();
        // First line is the author epoch; skip the record if it is not a number.
        let time = match lines.next().and_then(|l| l.trim().parse::<i64>().ok()) {
            Some(t) => t,
            None => continue,
        };
        let mut files = Vec::new();
        for line in lines {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if let Some(change) = parse_numstat_line(line) {
                files.push(change);
            }
        }
        commits.push(CommitChanges { time, files });
    }
    commits
}

/// Parse one `<ins>\t<del>\t<path>` numstat row (`-` = binary → counted as 0).
fn parse_numstat_line(line: &str) -> Option<FileChange> {
    let mut cols = line.splitn(3, '\t');
    let ins = cols.next()?;
    let del = cols.next()?;
    let path = cols.next()?;
    if path.is_empty() {
        return None;
    }
    Some(FileChange {
        path: path.to_string(),
        insertions: ins.parse::<u64>().unwrap_or(0),
        deletions: del.parse::<u64>().unwrap_or(0),
    })
}

/// Count current lines for each path that appears in `commits`, reading the
/// working tree. Missing / deleted paths are simply absent (the domain treats
/// an absent LOC as zero complexity). Bytes are counted, so binary or non-UTF-8
/// files don't break the scan.
fn scan_loc(repo_dir: &Path, commits: &[CommitChanges]) -> std::collections::BTreeMap<String, u32> {
    let mut loc = std::collections::BTreeMap::new();
    for c in commits {
        for f in &c.files {
            if loc.contains_key(&f.path) {
                continue;
            }
            if let Ok(bytes) = std::fs::read(repo_dir.join(&f.path)) {
                let n = bytecount_lines(&bytes);
                loc.insert(f.path.clone(), n);
            }
        }
    }
    loc
}

/// Lines = newline count, plus one for a final non-empty line lacking a
/// trailing newline (matches the intuitive "line count" of an editor).
fn bytecount_lines(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let trailing = if bytes.last() == Some(&b'\n') { 0 } else { 1 };
    (newlines + trailing).min(u32::MAX as usize) as u32
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure parsing — no git invocation)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_commits_with_files() {
        let stdout = format!(
            "{RS}1700000100\n\n12\t4\tsrc/a.rs\n1\t0\tsrc/b.rs\n{RS}1700000000\n\n3\t0\tsrc/a.rs\n"
        );
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].time, 1_700_000_100);
        assert_eq!(commits[0].files.len(), 2);
        assert_eq!(commits[0].files[0].path, "src/a.rs");
        assert_eq!(commits[0].files[0].insertions, 12);
        assert_eq!(commits[0].files[0].deletions, 4);
        assert_eq!(commits[1].files.len(), 1);
    }

    #[test]
    fn binary_rows_count_as_zero() {
        let stdout = format!("{RS}1700000000\n\n-\t-\tassets/logo.png\n");
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits[0].files[0].insertions, 0);
        assert_eq!(commits[0].files[0].deletions, 0);
        assert_eq!(commits[0].files[0].path, "assets/logo.png");
    }

    #[test]
    fn merge_commit_with_no_numstat_yields_empty_files() {
        let stdout = format!("{RS}1700000000\n");
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits.len(), 1);
        assert!(commits[0].files.is_empty());
    }

    #[test]
    fn ignores_garbage_records() {
        let stdout = format!("{RS}not-a-number\n5\t5\tx\n{RS}1700000000\n\n1\t1\ty\n");
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].files[0].path, "y");
    }

    #[test]
    fn line_counting() {
        assert_eq!(bytecount_lines(b""), 0);
        assert_eq!(bytecount_lines(b"a\nb\nc\n"), 3);
        assert_eq!(bytecount_lines(b"a\nb\nc"), 3); // no trailing newline
        assert_eq!(bytecount_lines(b"single"), 1);
    }
}
