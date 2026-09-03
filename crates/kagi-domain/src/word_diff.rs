//! Intra-line (word-level) diff — issue #349 §4 "word diff / intra-line".
//!
//! Pure, git2-free, gpui-free. Given the old and new text of a modify pair,
//! [`word_diff`] returns the byte ranges that actually changed on each side, so
//! the renderer can highlight only the changed words instead of painting the
//! whole line. Token granularity: runs of alphanumeric/`_` are one token, every
//! other char (punctuation, each whitespace char) is its own token. A
//! longest-common-subsequence over the tokens tells us what survived; the rest
//! coalesces into contiguous [`Span`]s.

use std::ops::Range;

/// Which side of a modify pair a changed span belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The old (left / removed) line.
    Old,
    /// The new (right / added) line.
    New,
}

/// A changed byte range within one side of a modify pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Which line the range indexes into.
    pub side: Side,
    /// Byte range within that line (always on char boundaries).
    pub range: Range<usize>,
}

/// One token: a byte range within its source string plus its text.
struct Token<'a> {
    range: Range<usize>,
    text: &'a str,
}

/// Split `s` into tokens: maximal runs of `[A-Za-z0-9_]` are one token, every
/// other char stands alone. Byte ranges index back into `s`.
fn tokenize(s: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((start, c)) = it.next() {
        if is_word(c) {
            let mut end = start + c.len_utf8();
            while let Some(&(i, nc)) = it.peek() {
                if is_word(nc) {
                    end = i + nc.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            out.push(Token {
                range: start..end,
                text: &s[start..end],
            });
        } else {
            let end = start + c.len_utf8();
            out.push(Token {
                range: start..end,
                text: &s[start..end],
            });
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Compute the changed spans between `old` and `new` at word granularity.
///
/// - identical input → no spans;
/// - a single changed token → one span covering just that token;
/// - a full rewrite (no shared tokens) → one span per side covering the whole
///   line.
pub fn word_diff(old: &str, new: &str) -> Vec<Span> {
    if old == new {
        return Vec::new();
    }
    let ot = tokenize(old);
    let nt = tokenize(new);

    // LCS over token text → which token indices are common on each side.
    let (old_common, new_common) = lcs_flags(&ot, &nt);

    let mut spans = Vec::new();
    push_changed(&mut spans, Side::Old, &ot, &old_common);
    push_changed(&mut spans, Side::New, &nt, &new_common);
    spans
}

/// Coalesce runs of non-common tokens into spans for one side.
fn push_changed(out: &mut Vec<Span>, side: Side, toks: &[Token<'_>], common: &[bool]) {
    let mut i = 0;
    while i < toks.len() {
        if common[i] {
            i += 1;
            continue;
        }
        let start = toks[i].range.start;
        let mut end = toks[i].range.end;
        i += 1;
        while i < toks.len() && !common[i] {
            end = toks[i].range.end;
            i += 1;
        }
        out.push(Span {
            side,
            range: start..end,
        });
    }
}

/// Standard LCS DP over token text; returns `(old_common, new_common)` boolean
/// masks marking which tokens are part of a longest common subsequence.
fn lcs_flags(ot: &[Token<'_>], nt: &[Token<'_>]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (ot.len(), nt.len());
    // dp[i][j] = LCS length of ot[i..] and nt[j..].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if ot[i].text == nt[j].text {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut old_common = vec![false; n];
    let mut new_common = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if ot[i].text == nt[j].text {
            old_common[i] = true;
            new_common[j] = true;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (old_common, new_common)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn news(spans: &[Span]) -> Vec<Range<usize>> {
        spans
            .iter()
            .filter(|s| s.side == Side::New)
            .map(|s| s.range.clone())
            .collect()
    }
    fn olds(spans: &[Span]) -> Vec<Range<usize>> {
        spans
            .iter()
            .filter(|s| s.side == Side::Old)
            .map(|s| s.range.clone())
            .collect()
    }

    #[test]
    fn identical_has_no_spans() {
        assert!(word_diff("let x = 1;", "let x = 1;").is_empty());
    }

    #[test]
    fn single_token_change_is_one_small_span() {
        // Only the middle token changes; the span must cover exactly `qux`,
        // not the whole line. (Mutation guard on the tokenizer boundary: a
        // whole-line token would make this the whole string.)
        let s = word_diff("foo bar baz", "foo qux baz");
        assert_eq!(news(&s), vec![4..7]);
        assert_eq!(olds(&s), vec![4..7]);
    }

    #[test]
    fn one_char_word_edit_highlights_only_that_word() {
        // `is_char_boundary`-safe; word granularity → the changed word only.
        let s = word_diff("value = old;", "value = new;");
        assert_eq!(&"value = new;"[news(&s)[0].clone()], "new");
        assert_eq!(&"value = old;"[olds(&s)[0].clone()], "old");
    }

    #[test]
    fn full_rewrite_spans_the_whole_line() {
        // No shared token (not even a space) → each side is one whole span.
        let old = "aaabbb";
        let new = "xxxyyyzzz";
        let s = word_diff(old, new);
        assert_eq!(olds(&s), vec![0..old.len()]);
        assert_eq!(news(&s), vec![0..new.len()]);
    }

    #[test]
    fn pure_insertion_only_marks_new_side() {
        // "a c" -> "a b c": an inserted word appears new-only; the old side is
        // untouched. (The exact coalesced span may swallow an adjacent space.)
        let s = word_diff("a c", "a b c");
        assert!(olds(&s).is_empty());
        let n = news(&s);
        assert_eq!(n.len(), 1);
        assert_eq!("a b c"[n[0].clone()].trim(), "b");
    }

    #[test]
    fn trailing_change_span_boundary_is_tight() {
        // Mutation guard: the boundary between shared prefix and the changed
        // suffix must be exact.
        let s = word_diff("call(a, b)", "call(a, c)");
        assert_eq!(&"call(a, c)"[news(&s)[0].clone()], "c");
    }
}
