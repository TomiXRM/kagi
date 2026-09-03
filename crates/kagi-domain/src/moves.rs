//! Move detection — issue #349 §4 "move detection".
//!
//! Pure, git2-free, gpui-free. Given the removed and added lines of a diff (in
//! diff order), [`detect_moves`] finds blocks that were removed in one place and
//! re-added in another — a refactor's "move", which git's `--color-moved` shows
//! so a moved block does not read as a giant delete plus a giant add.
//!
//! Approximates git `--color-moved` with `--color-moved-ws=allow-indentation-change`:
//! a greedy match of contiguous line runs, comparing lines after trimming
//! leading whitespace, and only accepting a block whose moved content carries at
//! least [`MOVE_MIN_ALNUM`] alphanumeric characters — the same "≥ 20 alnum"
//! floor git uses to avoid flagging trivial lines (`}`, `return;`) as moves.

use std::ops::Range;

/// Minimum alphanumeric characters in a block for it to count as a move
/// (git `--color-moved` uses the same floor). Below this a coincidental line
/// match is not treated as a move.
pub const MOVE_MIN_ALNUM: usize = 20;

/// A block that was removed in one place and added in another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedBlock {
    /// Contiguous index range into the `removed` slice.
    pub removed: Range<usize>,
    /// Contiguous index range into the `added` slice.
    pub added: Range<usize>,
}

/// Detect moved blocks between the diff's removed and added lines.
///
/// Greedy: for each still-unclaimed added line, find the longest contiguous run
/// of removed lines that matches from there, take the longest such run, and — if
/// it clears the [`MOVE_MIN_ALNUM`] floor — record it and consume those removed
/// lines. Lines are compared after `trim_start` (indentation-insensitive);
/// blank lines never match.
pub fn detect_moves(removed: &[&str], added: &[&str]) -> Vec<MovedBlock> {
    let mut moves = Vec::new();
    let mut used_removed = vec![false; removed.len()];
    let mut a = 0;
    while a < added.len() {
        let mut best: Option<(usize, usize)> = None; // (removed_start, len)
        for r in 0..removed.len() {
            if used_removed[r] || !lines_match(added[a], removed[r]) {
                continue;
            }
            let mut len = 0;
            while a + len < added.len()
                && r + len < removed.len()
                && !used_removed[r + len]
                && lines_match(added[a + len], removed[r + len])
            {
                len += 1;
            }
            if best.is_none_or(|(_, bl)| len > bl) {
                best = Some((r, len));
            }
        }
        if let Some((r, len)) = best {
            if alnum_count(&added[a..a + len]) >= MOVE_MIN_ALNUM {
                for k in 0..len {
                    used_removed[r + k] = true;
                }
                moves.push(MovedBlock {
                    removed: r..r + len,
                    added: a..a + len,
                });
                a += len;
                continue;
            }
        }
        a += 1;
    }
    moves
}

/// Two lines match for move purposes when they are equal after trimming leading
/// whitespace and are not blank.
fn lines_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_start(), b.trim_start());
    !a.is_empty() && a == b
}

fn alnum_count(lines: &[&str]) -> usize {
    lines
        .iter()
        .map(|l| l.chars().filter(|c| c.is_alphanumeric()).count())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A line with plenty of alphanumeric content (well over the floor).
    const BIG: &str = "let computed_result = expensive_calculation(input);";

    #[test]
    fn block_moved_down_is_detected_as_a_move() {
        let removed = &[BIG];
        let added = &["unrelated();", BIG];
        let moves = detect_moves(removed, added);
        assert_eq!(
            moves,
            vec![MovedBlock {
                removed: 0..1,
                added: 1..2
            }]
        );
    }

    #[test]
    fn multi_line_block_moves_as_one_block() {
        let a = "fn helper(value: usize) -> usize {";
        let b = "    value.wrapping_mul(2).saturating_add(1)";
        let removed = &[a, b];
        let added = &["noise();", a, b];
        let moves = detect_moves(removed, added);
        assert_eq!(
            moves,
            vec![MovedBlock {
                removed: 0..2,
                added: 1..3
            }]
        );
    }

    #[test]
    fn short_block_below_threshold_is_not_flagged() {
        // Genuinely moved, but only 5 alnum chars → under MOVE_MIN_ALNUM.
        // Mutation guard: dropping the ≥20 floor would flag this.
        let removed = &["let x = a;"];
        let added = &["other();", "let x = a;"];
        assert!(detect_moves(removed, added).is_empty());
    }

    #[test]
    fn delete_and_unrelated_add_is_not_a_move() {
        let removed = &["let removed_thing = compute_something_here();"];
        let added = &["let added_thing = a_totally_different_call();"];
        assert!(detect_moves(removed, added).is_empty());
    }

    #[test]
    fn genuine_rewrite_is_not_all_moved() {
        // Every line differs between sides → nothing matches → no moves.
        let removed = &[
            "the quick brown fox jumped over",
            "the lazy dog near the river",
        ];
        let added = &[
            "a completely different first sentence here",
            "and a second one with other words entirely",
        ];
        assert!(detect_moves(removed, added).is_empty());
    }

    #[test]
    fn indentation_change_still_matches() {
        // allow-indentation-change: same content, different leading whitespace.
        let removed = &[BIG];
        let added = &["x();", &format!("        {BIG}")];
        let refs: Vec<&str> = added.iter().map(|s| s.as_ref()).collect();
        let moves = detect_moves(removed, &refs);
        assert_eq!(moves.len(), 1);
    }

    #[test]
    fn blank_lines_do_not_anchor_moves() {
        let removed = &["   ", ""];
        let added = &["", "   "];
        assert!(detect_moves(removed, added).is_empty());
    }
}
