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
//! - `--format=%x1e%at%x1f%ae` prefixes each commit with `\x1e` (record
//!   separator) then the author time (epoch secs), a `\x1f` unit separator, and
//!   the author email.
//! - `--numstat` then lists `<ins>\t<del>\t<path>` rows (`-` for binary).
//! - `--no-renames` keeps every path plain (a rename reads as delete + add,
//!   which is fine for churn counting) and avoids the `{old => new}` numstat
//!   form.
//! - Merge commits emit no numstat rows by default → they contribute no file
//!   churn, which is the desired behaviour.
//! - `-c core.quotePath=false` keeps non-ASCII paths as raw UTF-8.

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::cli::run_git;
use super::GitError;
use kagi_domain::hotspot::{CommitChanges, FileChange, RawEcosystem};

/// Record separator emitted by `--format=%x1e…` before each commit.
const RS: char = '\u{1e}';
/// Unit separator between the author time and author email in the header line.
const US: char = '\u{1f}';

/// Parameters for a whole-repo ecosystem scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EcosystemRequest {
    /// Working-tree root of the repository.
    pub repo_dir: PathBuf,
    /// Maximum number of commits to mine (`git log -n`); `0` means unlimited.
    pub limit: usize,
    /// Exclude patterns in **gitignore syntax** (one per entry) — the sole
    /// source of exclusions (no built-in defaults). A file matching any pattern
    /// is dropped from the analysis. Sourced from the user's `analyze_ignore`
    /// config file.
    pub ignore_patterns: Vec<String>,
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
        "--format=%x1e%at%x1f%ae",
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

    let mut commits = parse_numstat_log(&out.stdout);
    // Drop excluded files (gitignore-format patterns) at the mining boundary so
    // the cache stays small and we never read their bytes for the LOC scan.
    let matcher = build_matcher(&req.repo_dir, &req.ignore_patterns);
    for c in &mut commits {
        c.files.retain(|f| {
            !matcher
                .matched(req.repo_dir.join(&f.path), false)
                .is_ignore()
        });
    }
    let loc = scan_loc(&req.repo_dir, &commits);
    Ok(RawEcosystem { commits, loc })
}

/// Compile the user's gitignore-format patterns into a matcher rooted at the
/// repository. Individual unparsable lines are skipped; a wholly invalid set
/// yields an empty matcher (excludes nothing).
fn build_matcher(repo_dir: &Path, patterns: &[String]) -> Gitignore {
    let mut b = GitignoreBuilder::new(repo_dir);
    for line in patterns {
        let _ = b.add_line(None, line);
    }
    b.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Parse `git log --numstat --format=%x1e%at` output into per-commit changes.
fn parse_numstat_log(stdout: &str) -> Vec<CommitChanges> {
    let mut commits = Vec::new();
    for record in stdout.split(RS) {
        if record.trim().is_empty() {
            continue;
        }
        let mut lines = record.lines();
        // Header line: `<epoch>\x1f<author-email>`. Skip if epoch isn't a number.
        let header = lines.next().unwrap_or("");
        let (time_str, author) = header.split_once(US).unwrap_or((header, ""));
        let time = match time_str.trim().parse::<i64>() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let author = author.to_string();
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
        commits.push(CommitChanges {
            time,
            author,
            files,
        });
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
    fn parses_author_email_from_header() {
        let stdout = format!("{RS}1700000100{US}alice@x\n\n3\t0\tsrc/a.rs\n");
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits[0].time, 1_700_000_100);
        assert_eq!(commits[0].author, "alice@x");
        assert_eq!(commits[0].files[0].path, "src/a.rs");
    }

    #[test]
    fn binary_rows_count_as_zero() {
        // A non-excluded binary blob (`-`/`-` numstat) → counted, with 0 lines.
        let stdout = format!("{RS}1700000000\n\n-\t-\tdata/blob.bin\n");
        let commits = parse_numstat_log(&stdout);
        assert_eq!(commits[0].files[0].insertions, 0);
        assert_eq!(commits[0].files[0].deletions, 0);
        assert_eq!(commits[0].files[0].path, "data/blob.bin");
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
    fn gitignore_matcher_supports_wildcards_and_names() {
        let root = Path::new("/repo");
        let patterns = vec![
            "*.pdf".to_string(),
            "*.kicad_*".to_string(),
            "fonts/**".to_string(),
            "fp-info-cache".to_string(),
            "# a comment".to_string(),
        ];
        let gi = build_matcher(root, &patterns);
        let ignored = |rel: &str| gi.matched(root.join(rel), false).is_ignore();
        assert!(ignored("doc/manual.pdf"));
        assert!(ignored("board.kicad_pcb"));
        assert!(ignored("fonts/Inter.ttf"));
        assert!(ignored("hw/proj/fp-info-cache"));
        assert!(!ignored("src/main.rs"));
        assert!(!ignored("README.md"));
        // No patterns → nothing excluded.
        let empty = build_matcher(root, &[]);
        assert!(!empty.matched(root.join("any.pdf"), false).is_ignore());
    }

    #[test]
    fn line_counting() {
        assert_eq!(bytecount_lines(b""), 0);
        assert_eq!(bytecount_lines(b"a\nb\nc\n"), 3);
        assert_eq!(bytecount_lines(b"a\nb\nc"), 3); // no trailing newline
        assert_eq!(bytecount_lines(b"single"), 1);
    }
}
