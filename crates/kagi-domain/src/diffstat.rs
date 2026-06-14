//! Per-file diffstat domain model and pure bar allocation.
//!
//! The git2-backed diffstat aggregation functions live in the git-backend
//! layer (`kagi::git::diffstat`).

use std::path::{Path, PathBuf};

use crate::status::ChangeKind;

/// Per-file additions/deletions summary for the diffstat mini bar.
///
/// UI-independent pure data.  `change` reuses [`ChangeKind`] (spec allows it);
/// for a renamed file `change` is [`ChangeKind::Renamed`] carrying the old path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiffStat {
    /// New-side path of the file (old path for a pure deletion).
    pub path: PathBuf,
    /// The kind of change (Added / Modified / Deleted / Renamed / TypeChange).
    pub change: ChangeKind,
    /// Number of added lines (0 for binary).
    pub additions: usize,
    /// Number of deleted lines (0 for binary).
    pub deletions: usize,
    /// `true` when git detected the file as binary (no line stats).
    pub is_binary: bool,
}

impl FileDiffStat {
    /// Total changed lines (`additions + deletions`).
    pub fn total(&self) -> usize {
        self.additions + self.deletions
    }
}

/// Allocate `max_segments` green/red bar segments from `additions`/`deletions`.
///
/// Returns `(green, red)` segment counts with `green + red <= max_segments`.
///
/// Rules (spec §計算仕様):
/// * `total == 0` → `(0, 0)` (UI shows a placeholder).
/// * If a side has any lines it is guaranteed **at least 1 segment** so small
///   changes stay visible.
/// * The remaining segments are distributed by ratio; rounding never exceeds
///   `max_segments`.
pub fn bar_segments(additions: usize, deletions: usize, max_segments: usize) -> (usize, usize) {
    let total = additions + deletions;
    if total == 0 || max_segments == 0 {
        return (0, 0);
    }

    // Only one side present → all segments to that side.
    if additions == 0 {
        return (0, max_segments);
    }
    if deletions == 0 {
        return (max_segments, 0);
    }

    // Both sides present: each side is guaranteed at least one segment, so the
    // remaining `max_segments - 2` are distributed by additions ratio.
    if max_segments == 1 {
        // Degenerate: a single segment can't show both. Give it to the
        // majority side (ties → green/additions).
        return if additions >= deletions {
            (1, 0)
        } else {
            (0, 1)
        };
    }

    let extra = max_segments - 2;
    // green_extra = round(extra * additions / total)
    let green_extra = (extra * additions + total / 2) / total;
    let green = 1 + green_extra;
    let red = max_segments - green;
    (green, red)
}

/// Find the [`FileDiffStat`] for `path` in `stats` (by new-side path).
pub fn find_stat<'a>(stats: &'a [FileDiffStat], path: &Path) -> Option<&'a FileDiffStat> {
    stats.iter().find(|s| s.path == path)
}

// ────────────────────────────────────────────────────────────
// Unit tests — bar_segments (T-DIFFSTAT-003 fixed examples)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: usize = 5;

    #[test]
    fn additions_only_all_green() {
        // +10 -0 → green only
        assert_eq!(bar_segments(10, 0, MAX), (MAX, 0));
    }

    #[test]
    fn deletions_only_all_red() {
        // +0 -10 → red only
        assert_eq!(bar_segments(0, 10, MAX), (0, MAX));
    }

    #[test]
    fn balanced_half_each() {
        // +5 -5 → green and red half-and-half
        let (g, r) = bar_segments(5, 5, MAX);
        assert_eq!(g + r, MAX);
        // With max=5: 1 guaranteed each + round(3*5/10)=2 extra green ⇒ (3,2).
        assert_eq!((g, r), (3, 2));
        // Symmetric within one segment.
        assert!(g.abs_diff(r) <= 1);
    }

    #[test]
    fn tiny_addition_many_deletions_min_one_green() {
        // +1 -20 → at least 1 green segment, rest red.
        let (g, r) = bar_segments(1, 20, MAX);
        assert_eq!(g, 1, "minimum 1 green segment must be guaranteed");
        assert_eq!(r, MAX - 1);
    }

    #[test]
    fn many_additions_tiny_deletion_min_one_red() {
        // +200 -10 → mostly green but at least 1 red segment.
        let (g, r) = bar_segments(200, 10, MAX);
        assert_eq!(r, 1, "minimum 1 red segment must be guaranteed");
        assert_eq!(g, MAX - 1);
    }

    #[test]
    fn zero_total_is_empty() {
        assert_eq!(bar_segments(0, 0, MAX), (0, 0));
    }

    #[test]
    fn never_exceeds_max() {
        for a in 0..50 {
            for d in 0..50 {
                for m in 1..=8 {
                    let (g, r) = bar_segments(a, d, m);
                    assert!(g + r <= m, "g+r exceeds max for ({a},{d},{m})");
                    if a + d > 0 && a > 0 && m >= 2 && d > 0 {
                        assert!(
                            g >= 1,
                            "green must be >=1 when additions exist ({a},{d},{m})"
                        );
                        assert!(r >= 1, "red must be >=1 when deletions exist ({a},{d},{m})");
                    }
                }
            }
        }
    }
}
