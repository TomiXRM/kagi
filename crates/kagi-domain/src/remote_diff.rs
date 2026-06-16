//! Pure parsers for a remote commit's changed files and unified file diff
//! (ADR-0089, Phase 2c). No I/O, no `git2`.
//!
//! The I/O layer (`kagi::remote`) runs `git diff-tree --name-status` and
//! `git show … -- <path>` over SSH; these parsers turn that text into the same
//! [`FileStatus`]/[`FileDiff`] domain types the local `git2` diff produces, so
//! the existing diff views render a remote commit unchanged. Unit-testable from
//! literal `git` output.

use std::path::PathBuf;

use crate::diff::{DiffLine, DiffLineKind, FileDiff, Hunk};
use crate::status::{ChangeKind, FileStatus};

// ── Changed files (`git diff-tree --name-status -M`) ─────────────

/// Parse `git diff-tree --no-commit-id -r -M --root --name-status <sha>` output
/// (tab-separated `STATUS\tPATH`, or `Rxxx\tOLD\tNEW` for renames/copies).
pub fn parse_name_status(text: &str) -> Vec<FileStatus> {
    text.lines().filter_map(parse_name_status_line).collect()
}

fn parse_name_status_line(line: &str) -> Option<FileStatus> {
    if line.trim().is_empty() {
        return None;
    }
    let mut f = line.split('\t');
    let code = f.next()?.trim();
    let c0 = code.chars().next()?;
    let (path, change) = match c0 {
        'A' => (f.next()?, ChangeKind::Added),
        'D' => (f.next()?, ChangeKind::Deleted),
        'T' => (f.next()?, ChangeKind::TypeChange),
        'R' => {
            let from = f.next()?;
            let new = f.next()?;
            (
                new,
                ChangeKind::Renamed {
                    from: PathBuf::from(from),
                },
            )
        }
        // Copy: report the new path as a plain modification (matches the local
        // backend's "exotic status → Modified" default).
        'C' => {
            let _from = f.next()?;
            (f.next()?, ChangeKind::Modified)
        }
        // 'M' and anything else: a single trailing path, modified.
        _ => (f.next()?, ChangeKind::Modified),
    };
    Some(FileStatus {
        path: PathBuf::from(path),
        change,
    })
}

// ── Unified file diff (`git show --format= <sha> -- <path>`) ──────

/// Parse a single-file unified diff (`git show`/`git diff` output) into a
/// [`FileDiff`]. Tolerant of the `diff --git` / mode / rename / `index` header
/// lines; reads paths from the `---`/`+++` lines and the change kind from the
/// `new file` / `deleted file` / `rename` / `/dev/null` markers. `Binary files …
/// differ` sets `is_binary` with no hunks.
pub fn parse_file_diff(text: &str) -> FileDiff {
    let mut old_path: Option<PathBuf> = None;
    let mut new_path: Option<PathBuf> = None;
    let mut change = ChangeKind::Modified;
    let mut is_binary = false;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut cur: Option<Hunk> = None;
    let mut old_no = 0u32;
    let mut new_no = 0u32;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("rename from ") {
            old_path = Some(PathBuf::from(rest));
            change = ChangeKind::Renamed {
                from: PathBuf::from(rest),
            };
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            new_path = Some(PathBuf::from(rest));
            continue;
        }
        if line.starts_with("new file mode") {
            change = ChangeKind::Added;
            continue;
        }
        if line.starts_with("deleted file mode") {
            change = ChangeKind::Deleted;
            continue;
        }
        if line.starts_with("Binary files ") {
            is_binary = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("--- ") {
            if rest == "/dev/null" {
                change = ChangeKind::Added;
            } else {
                old_path = Some(PathBuf::from(strip_ab(rest)));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            if rest == "/dev/null" {
                change = ChangeKind::Deleted;
            } else {
                new_path = Some(PathBuf::from(strip_ab(rest)));
            }
            continue;
        }
        if line.starts_with("@@") {
            if let Some(h) = cur.take() {
                hunks.push(h);
            }
            if let Some((os, _oc, ns, _nc)) = parse_hunk_header(line) {
                old_no = os;
                new_no = ns;
                cur = Some(Hunk {
                    old_range: (os, _oc),
                    new_range: (ns, _nc),
                    lines: Vec::new(),
                });
            }
            continue;
        }
        // Skip the remaining header lines (`diff --git`, `index`, mode lines)
        // until the first hunk opens.
        let Some(h) = cur.as_mut() else { continue };
        let mut chars = line.chars();
        match chars.next() {
            Some(' ') => {
                h.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    content: chars.as_str().to_string(),
                    old_lineno: Some(old_no),
                    new_lineno: Some(new_no),
                });
                old_no += 1;
                new_no += 1;
            }
            Some('+') => {
                h.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    content: chars.as_str().to_string(),
                    old_lineno: None,
                    new_lineno: Some(new_no),
                });
                new_no += 1;
            }
            Some('-') => {
                h.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    content: chars.as_str().to_string(),
                    old_lineno: Some(old_no),
                    new_lineno: None,
                });
                old_no += 1;
            }
            // `\ No newline at end of file` and any stray blank lines: ignore.
            _ => {}
        }
    }
    if let Some(h) = cur.take() {
        hunks.push(h);
    }

    FileDiff {
        old_path,
        new_path,
        change,
        hunks,
        is_binary,
    }
}

/// Strip a leading `a/` or `b/` from a `---`/`+++` path.
fn strip_ab(p: &str) -> &str {
    p.strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p)
}

/// Parse `@@ -oldStart[,oldCount] +newStart[,newCount] @@ …` → counts default 1.
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let inner = line.strip_prefix("@@ ")?;
    let inner = inner.split(" @@").next()?;
    let mut parts = inner.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let (os, oc) = parse_range(old)?;
    let (ns, nc) = parse_range(new)?;
    Some((os, oc, ns, nc))
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_status_variants() {
        let out = "M\tsrc/a.rs\nA\tsrc/new.rs\nD\told.rs\nR100\tfrom.rs\tto.rs\nT\tlink";
        let fs = parse_name_status(out);
        assert_eq!(fs.len(), 5);
        assert_eq!(fs[0].path, PathBuf::from("src/a.rs"));
        assert_eq!(fs[0].change, ChangeKind::Modified);
        assert_eq!(fs[1].change, ChangeKind::Added);
        assert_eq!(fs[2].change, ChangeKind::Deleted);
        assert_eq!(fs[3].path, PathBuf::from("to.rs"));
        assert_eq!(
            fs[3].change,
            ChangeKind::Renamed {
                from: PathBuf::from("from.rs")
            }
        );
        assert_eq!(fs[4].change, ChangeKind::TypeChange);
    }

    #[test]
    fn file_diff_modified_with_hunk() {
        // NB: build with an array join — a `\`-continued string literal would
        // strip the leading space off context lines (" keep").
        let out = [
            "diff --git a/f.txt b/f.txt",
            "index 111..222 100644",
            "--- a/f.txt",
            "+++ b/f.txt",
            "@@ -1,3 +1,4 @@",
            " keep",
            "-old line",
            "+new line",
            "+added line",
            " tail",
        ]
        .join("\n");
        let d = parse_file_diff(&out);
        assert!(!d.is_binary);
        assert_eq!(d.change, ChangeKind::Modified);
        assert_eq!(d.new_path, Some(PathBuf::from("f.txt")));
        assert_eq!(d.hunks.len(), 1);
        let h = &d.hunks[0];
        assert_eq!(h.old_range, (1, 3));
        assert_eq!(h.new_range, (1, 4));
        let kinds: Vec<_> = h.lines.iter().map(|l| l.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Context,
                DiffLineKind::Removed,
                DiffLineKind::Added,
                DiffLineKind::Added,
                DiffLineKind::Context,
            ]
        );
        // line-number tracking
        assert_eq!(h.lines[0].old_lineno, Some(1));
        assert_eq!(h.lines[0].new_lineno, Some(1));
        assert_eq!(h.lines[1].old_lineno, Some(2)); // removed: old advances
        assert_eq!(h.lines[1].new_lineno, None);
        assert_eq!(h.lines[2].new_lineno, Some(2)); // added: new advances
        assert_eq!(h.lines[4].new_lineno, Some(4));
    }

    #[test]
    fn file_diff_added_and_deleted() {
        let added = [
            "diff --git a/n.rs b/n.rs",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/n.rs",
            "@@ -0,0 +1,2 @@",
            "+line1",
            "+line2",
        ]
        .join("\n");
        let d = parse_file_diff(&added);
        assert_eq!(d.change, ChangeKind::Added);
        assert_eq!(d.hunks[0].lines.len(), 2);

        let deleted = [
            "diff --git a/g.rs b/g.rs",
            "deleted file mode 100644",
            "--- a/g.rs",
            "+++ /dev/null",
            "@@ -1,1 +0,0 @@",
            "-gone",
        ]
        .join("\n");
        let d = parse_file_diff(&deleted);
        assert_eq!(d.change, ChangeKind::Deleted);
        assert_eq!(d.old_path, Some(PathBuf::from("g.rs")));
    }

    #[test]
    fn file_diff_binary() {
        let out = [
            "diff --git a/i.png b/i.png",
            "index 1..2 100644",
            "Binary files a/i.png and b/i.png differ",
        ]
        .join("\n");
        let d = parse_file_diff(&out);
        assert!(d.is_binary);
        assert!(d.hunks.is_empty());
    }

    #[test]
    fn hunk_header_default_counts() {
        assert_eq!(parse_hunk_header("@@ -5 +7 @@"), Some((5, 1, 7, 1)));
        assert_eq!(
            parse_hunk_header("@@ -1,0 +1,3 @@ fn x()"),
            Some((1, 0, 1, 3))
        );
    }
}
