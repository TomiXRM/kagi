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

/// Lines longer than this (bytes, either side) skip the LCS entirely
/// (issue #398): punctuation-dense text tokenizes to ~1 token per char, so a
/// 100 KB minified line would cost O(n·m) DP cells. Above the cap the pair is
/// treated as a full rewrite — whole-line emphasis, no intra-line highlight.
pub const MAX_WORD_DIFF_LEN: usize = 4096;

/// Compute the changed spans between `old` and `new` at word granularity.
///
/// - identical input → no spans;
/// - a single changed token → one span covering just that token;
/// - a full rewrite (no shared tokens) → one span per side covering the whole
///   line;
/// - either side longer than [`MAX_WORD_DIFF_LEN`] bytes → whole-line spans
///   without running the LCS (issue #398).
pub fn word_diff(old: &str, new: &str) -> Vec<Span> {
    if old == new {
        return Vec::new();
    }
    if old.len().max(new.len()) > MAX_WORD_DIFF_LEN {
        let mut spans = Vec::new();
        for (side, s) in [(Side::Old, old), (Side::New, new)] {
            if !s.is_empty() {
                spans.push(Span {
                    side,
                    range: 0..s.len(),
                });
            }
        }
        return spans;
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

/// LCS over token text; returns `(old_common, new_common)` boolean masks
/// marking which tokens are part of a longest common subsequence.
///
/// Hirschberg's algorithm (issue #398): O(n·m) time but O(min(n, m)) memory —
/// two rolling DP rows in flat `Vec<u32>`s instead of the previous dense
/// `Vec<Vec<u32>>` matrix.
fn lcs_flags(ot: &[Token<'_>], nt: &[Token<'_>]) -> (Vec<bool>, Vec<bool>) {
    let o: Vec<&str> = ot.iter().map(|t| t.text).collect();
    let n: Vec<&str> = nt.iter().map(|t| t.text).collect();
    let mut old_common = vec![false; o.len()];
    let mut new_common = vec![false; n.len()];
    // DP rows are sized by the second sequence — pass the shorter one there so
    // memory is O(min(n, m)). LCS is symmetric, so the masks just swap roles.
    if n.len() <= o.len() {
        hirschberg(&o, &n, 0, 0, &mut old_common, &mut new_common);
    } else {
        hirschberg(&n, &o, 0, 0, &mut new_common, &mut old_common);
    }
    (old_common, new_common)
}

/// Mark one LCS of `a[ao..]` vs `b[bo..]` in the `ac` / `bc` masks (indices
/// offset by `ao` / `bo` into the full sequences). Divide-and-conquer on `a`;
/// each level's split point comes from two rolling-row LCS length passes.
fn hirschberg(a: &[&str], b: &[&str], ao: usize, bo: usize, ac: &mut [bool], bc: &mut [bool]) {
    if a.is_empty() || b.is_empty() {
        return;
    }
    if a.len() == 1 {
        if let Some(j) = b.iter().position(|t| *t == a[0]) {
            ac[ao] = true;
            bc[bo + j] = true;
        }
        return;
    }
    let mid = a.len() / 2;
    let fwd = lcs_last_row(&a[..mid], b, false);
    let rev = lcs_last_row(&a[mid..], b, true);
    let split = (0..=b.len())
        .max_by_key(|&j| fwd[j] + rev[b.len() - j])
        .unwrap_or(0);
    hirschberg(&a[..mid], &b[..split], ao, bo, ac, bc);
    hirschberg(&a[mid..], &b[split..], ao + mid, bo + split, ac, bc);
}

/// Final DP row of LCS lengths between `a` and every prefix of `b`
/// (`rev` = both sequences reversed). Two rolling rows, flat allocations.
fn lcs_last_row(a: &[&str], b: &[&str], rev: bool) -> Vec<u32> {
    let mut prev = vec![0u32; b.len() + 1];
    let mut cur = vec![0u32; b.len() + 1];
    for i in 0..a.len() {
        let at = if rev { a[a.len() - 1 - i] } else { a[i] };
        for j in 0..b.len() {
            let bt = if rev { b[b.len() - 1 - j] } else { b[j] };
            cur[j + 1] = if at == bt {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev
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
    fn very_long_line_skips_the_lcs_and_spans_whole_lines() {
        // issue #398: above MAX_WORD_DIFF_LEN the LCS must not run at all —
        // a punctuation-dense 10 KB pair is ~10k tokens/side (~100M DP cells;
        // a real 100 KB minified line would abort on allocation). The cap
        // returns whole-line emphasis instead. Mutation guard: without the
        // cap the LCS would match the shared dots and emit only a tiny
        // new-side span (and take orders of magnitude longer).
        let old = ".".repeat(MAX_WORD_DIFF_LEN + 5_000);
        let new = format!("{old}!");
        let s = word_diff(&old, &new);
        assert_eq!(olds(&s), vec![0..old.len()]);
        assert_eq!(news(&s), vec![0..new.len()]);
    }

    #[test]
    fn trailing_change_span_boundary_is_tight() {
        // Mutation guard: the boundary between shared prefix and the changed
        // suffix must be exact.
        let s = word_diff("call(a, b)", "call(a, c)");
        assert_eq!(&"call(a, c)"[news(&s)[0].clone()], "c");
    }
}
