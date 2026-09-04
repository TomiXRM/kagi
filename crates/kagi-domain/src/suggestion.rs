//! PR review "suggested change" — pure parse + apply (#351, ADR-0172).
//!
//! A GitHub review comment may carry a ```suggestion fenced block: an
//! applyable code proposal. This module turns that block (plus the comment's
//! anchor) into a [`Suggestion`] and splices it into a working-tree file. It is
//! pure over its inputs — the git layer (`kagi-git`) reads the file, captures
//! the anchored lines for a TOCTOU stale guard, and writes.

/// A GitHub review "suggested change" resolved to a concrete working-tree edit.
///
/// The fenced ```suggestion block becomes `replacement` (empty = a deletion);
/// the target range is the review comment's anchor, `[start_line, end_line]`
/// (1-based inclusive). Pure data — applying it to a file is
/// [`Suggestion::apply_to`]; the git layer captures the pre-apply lines for a
/// TOCTOU stale-line guard before writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Repo-relative file the suggestion edits.
    pub path: String,
    /// First line replaced (1-based inclusive).
    pub start_line: u32,
    /// Last line replaced (1-based inclusive). Equal to `start_line` for a
    /// single-line suggestion.
    pub end_line: u32,
    /// The replacement text (no trailing newline; joined with `\n`). Empty when
    /// the suggestion deletes the range.
    pub replacement: String,
}

impl Suggestion {
    /// Splice `replacement` into `original` (the whole file), replacing the
    /// 1-based inclusive `[start_line, end_line]` range. Returns the new file
    /// content, or `None` if the range is out of bounds / inverted. A trailing
    /// newline on `original` is preserved.
    pub fn apply_to(&self, original: &str) -> Option<String> {
        if self.start_line == 0 || self.start_line > self.end_line {
            return None;
        }
        let had_nl = original.ends_with('\n');
        let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
        let s = (self.start_line - 1) as usize;
        let e = self.end_line as usize;
        if s >= lines.len() || e > lines.len() {
            return None;
        }
        let repl: Vec<String> = if self.replacement.is_empty() {
            Vec::new()
        } else {
            self.replacement.lines().map(str::to_string).collect()
        };
        lines.splice(s..e, repl);
        let mut out = lines.join("\n");
        if had_nl && !out.is_empty() {
            out.push('\n');
        }
        Some(out)
    }
}

/// The 1-based inclusive `[start, end]` lines of `content`, or `None` when the
/// range is out of bounds / inverted. Used by the git layer to capture the
/// anchored lines at plan time and re-read them at execute time (stale guard).
pub fn line_range(content: &str, start: u32, end: u32) -> Option<Vec<String>> {
    if start == 0 || start > end {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let s = (start - 1) as usize;
    let e = end as usize;
    if s >= lines.len() || e > lines.len() {
        return None;
    }
    Some(lines[s..e].iter().map(|l| l.to_string()).collect())
}

/// Parse a GitHub ```suggestion block out of a review comment `body`, anchored
/// to the caller-supplied `path` / `start_line` / `line`. Pure over its inputs
/// (the anchor comes from the `gh` review data; this never touches `gh`).
///
/// Returns `None` when there is no ```suggestion fence or it is unterminated.
/// A fence with no content lines is a **deletion** → `Some` with an empty
/// `replacement`. Fence indentation (e.g. inside a Markdown list item) is
/// stripped from the content lines.
pub fn parse_suggestion(
    body: &str,
    path: &str,
    start_line: Option<u32>,
    line: u32,
) -> Option<Suggestion> {
    let all: Vec<&str> = body.lines().collect();
    let open = all
        .iter()
        .position(|l| l.trim_start().starts_with("```suggestion"))?;
    let indent_len = all[open].len() - all[open].trim_start().len();
    let indent = &all[open][..indent_len];

    let mut repl: Vec<String> = Vec::new();
    for l in &all[open + 1..] {
        if l.trim() == "```" {
            let end_line = line;
            let start_line = start_line.filter(|&s| s > 0).unwrap_or(line);
            return Some(Suggestion {
                path: path.to_string(),
                start_line,
                end_line,
                replacement: repl.join("\n"),
            });
        }
        repl.push(l.strip_prefix(indent).unwrap_or(l).to_string());
    }
    None // unterminated fence — not a valid suggestion
}

impl crate::github::ReviewComment {
    /// The applyable [`Suggestion`] this comment carries, anchored to its own
    /// `path` / `start_line` / `line`. `None` when the body has no
    /// ```suggestion block, or the anchor is outdated (`line == 0`). Pure glue
    /// over [`parse_suggestion`] — the parse itself is caller-input-driven and
    /// unit-tested independently of `gh`.
    pub fn suggestion(&self) -> Option<Suggestion> {
        if self.line == 0 {
            return None; // outdated anchor: no working-tree range to apply to
        }
        parse_suggestion(&self.body, &self.path, self.start_line, self.line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_suggestion_present_returns_replacement() {
        let s = parse_suggestion("nit\n```suggestion\nlet x = 1;\n```\n", "src/a.rs", None, 5)
            .expect("a fenced suggestion parses");
        assert_eq!(s.path, "src/a.rs");
        assert_eq!((s.start_line, s.end_line), (5, 5));
        assert_eq!(s.replacement, "let x = 1;");
    }

    #[test]
    fn parse_suggestion_absent_returns_none() {
        assert!(parse_suggestion("looks good to me", "src/a.rs", None, 5).is_none());
        // A plain code fence is not a suggestion.
        assert!(parse_suggestion("```rust\nlet x = 1;\n```", "src/a.rs", None, 5).is_none());
        // Unterminated fence is not applyable.
        assert!(parse_suggestion("```suggestion\nlet x = 1;", "src/a.rs", None, 5).is_none());
    }

    #[test]
    fn parse_suggestion_multi_line_spans_start_to_line() {
        let s = parse_suggestion("```suggestion\na\nb\nc\n```", "f", Some(3), 5)
            .expect("multi-line suggestion parses");
        assert_eq!((s.start_line, s.end_line), (3, 5));
        assert_eq!(s.replacement, "a\nb\nc");
    }

    #[test]
    fn parse_suggestion_empty_is_a_deletion() {
        let s = parse_suggestion("delete this\n```suggestion\n```", "f", None, 7)
            .expect("empty suggestion still parses");
        assert_eq!(s.replacement, "");
        assert_eq!((s.start_line, s.end_line), (7, 7));
    }

    #[test]
    fn apply_to_splices_the_anchored_range() {
        let file = "one\ntwo\nthree\n";
        // Replace line 2 ("two") with "TWO".
        let s = Suggestion {
            path: "f".into(),
            start_line: 2,
            end_line: 2,
            replacement: "TWO".into(),
        };
        assert_eq!(s.apply_to(file).unwrap(), "one\nTWO\nthree\n");

        // Multi-line replacement of lines 2..=3.
        let s = Suggestion {
            path: "f".into(),
            start_line: 2,
            end_line: 3,
            replacement: "TWO\nTHREE\nFOUR".into(),
        };
        assert_eq!(s.apply_to(file).unwrap(), "one\nTWO\nTHREE\nFOUR\n");

        // Deletion of line 2.
        let s = Suggestion {
            path: "f".into(),
            start_line: 2,
            end_line: 2,
            replacement: String::new(),
        };
        assert_eq!(s.apply_to(file).unwrap(), "one\nthree\n");

        // Out-of-bounds range refuses.
        let s = Suggestion {
            path: "f".into(),
            start_line: 9,
            end_line: 9,
            replacement: "x".into(),
        };
        assert!(s.apply_to(file).is_none());
    }
}
