//! Blame data layer — pure models + `.git-blame-ignore-revs` parser (ADR-0162).
//!
//! This module is the **pure** half of line-level blame (issue #350). It holds:
//!
//! - the per-line attribution models ([`BlameLine`], [`BlameResult`]) that the
//!   git2-backed `kagi_git::blame` populates, and
//! - [`parse_blame_ignore_revs`] — the text parser for a repository's
//!   `.git-blame-ignore-revs` file. git2's blame API has **no** native
//!   ignore-revs support, so Kagi parses the file itself (a pure text task,
//!   hence it lives here) and the git layer applies the resulting set.
//!
//! No `git2`, no `gpui`, no I/O — the file is read by the git layer and handed
//! to [`parse_blame_ignore_revs`] as a `&str`.
//!
//! # Markers (issue #350 / #354)
//!
//! Ignored and unblamable lines are distinguished by a **symbol**, never by
//! colour alone, so the distinction survives on a monochrome display and for
//! colour-blind users. See [`IGNORED_MARK`] / [`UNBLAMABLE_MARK`].

/// Marker shown next to a line whose attributed commit is listed in
/// `.git-blame-ignore-revs` (git's `blame.markIgnoredLines`, symbol `*`).
pub const IGNORED_MARK: char = '*';

/// Marker shown next to a line that could not be attributed to any commit
/// (git's `blame.markUnblamableLines`, symbol `?`) — e.g. an uncommitted line.
pub const UNBLAMABLE_MARK: char = '?';

/// One attributed line of a blamed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// 1-based line number in the final (blamed) file.
    pub line_no: usize,
    /// Full 40-hex commit id the line is attributed to (empty when unblamable).
    pub commit: String,
    /// Abbreviated commit id for display (empty when unblamable).
    pub short_commit: String,
    /// Author name.
    pub author: String,
    /// Author time in Unix epoch seconds (formatting is the UI's job, matching
    /// the `kagi_domain::commit::Signature` convention). `0` when unblamable.
    pub time: i64,
    /// Commit summary (first line of the message).
    pub summary: String,
    /// The attributed commit is listed in `.git-blame-ignore-revs`. v1 **marks**
    /// the line; full re-attribution past the ignored commit is a follow-up.
    pub ignored: bool,
    /// The line could not be attributed to a commit (rare — uncommitted line).
    pub unblamable: bool,
}

impl BlameLine {
    /// The marker symbol for this line, if any. Unblamable wins over ignored.
    /// Returns `None` for a normally-attributed line.
    pub fn mark(&self) -> Option<char> {
        if self.unblamable {
            Some(UNBLAMABLE_MARK)
        } else if self.ignored {
            Some(IGNORED_MARK)
        } else {
            None
        }
    }
}

/// The result of blaming a single file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlameResult {
    /// Per-line attribution, in file order (line 1 first).
    pub lines: Vec<BlameLine>,
    /// How many **distinct** ignore-revs commits actually took effect in this
    /// file (i.e. appear as a marked line). This is the number surfaced to the
    /// user as "N revisions ignored"; `0` means the ignore file was absent or
    /// none of its commits touched this file.
    pub ignored_revs: usize,
}

/// Parse a `.git-blame-ignore-revs` file body into its list of revisions.
///
/// Format (same as `git`'s): one commit id (full or abbreviated) per line;
/// blank lines are skipped; a line whose first non-space character is `#` is a
/// comment and is skipped. The first whitespace-delimited token of each
/// remaining line is taken as the revision, tolerating trailing whitespace.
///
/// The returned strings are lowercased so the git layer can prefix-match them
/// against full commit ids case-insensitively.
pub fn parse_blame_ignore_revs(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_sha_per_line() {
        let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
                    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n";
        assert_eq!(
            parse_blame_ignore_revs(text),
            vec![
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ]
        );
    }

    #[test]
    fn skips_comments_and_blanks() {
        let text = "# Reformat everything with rustfmt\n\
                    \n\
                    deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n\
                    \n\
                    # another note\n";
        assert_eq!(
            parse_blame_ignore_revs(text),
            vec!["deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string()]
        );
    }

    #[test]
    fn accepts_abbreviated_and_lowercases() {
        // Abbreviated + uppercase + indented + trailing whitespace/comment.
        let text = "   ABCDEF0   \n\t1234abcd\n";
        assert_eq!(
            parse_blame_ignore_revs(text),
            vec!["abcdef0".to_string(), "1234abcd".to_string()]
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_blame_ignore_revs("").is_empty());
        assert!(parse_blame_ignore_revs("\n\n  \n# only a comment\n").is_empty());
    }

    #[test]
    fn mark_prefers_unblamable_over_ignored() {
        let base = BlameLine {
            line_no: 1,
            commit: String::new(),
            short_commit: String::new(),
            author: String::new(),
            time: 0,
            summary: String::new(),
            ignored: false,
            unblamable: false,
        };
        assert_eq!(base.mark(), None);
        assert_eq!(
            BlameLine {
                ignored: true,
                ..base.clone()
            }
            .mark(),
            Some(IGNORED_MARK)
        );
        assert_eq!(
            BlameLine {
                ignored: true,
                unblamable: true,
                ..base
            }
            .mark(),
            Some(UNBLAMABLE_MARK)
        );
    }
}
