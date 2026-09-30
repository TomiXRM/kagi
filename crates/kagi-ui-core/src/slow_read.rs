//! #355: explaining a slow read (ADR-0086 amendment).
//!
//! A read that has run for [`SLOW_READ_THRESHOLD`] earns one line in the busy
//! snackbar naming what is slow and why, the way git's
//! `advice.statusAheadBehind` does after two seconds. The threshold is a
//! product constant, not a setting.

use std::time::Duration;

/// Git's own threshold for `advice.statusAheadBehind` / `advice.resetNoRefresh`.
pub const SLOW_READ_THRESHOLD: Duration = Duration::from_secs(2);

/// Whether a read that has run for `elapsed` is slow enough to explain.
pub fn is_slow(elapsed: Duration) -> bool {
    elapsed >= SLOW_READ_THRESHOLD
}

/// The reads that explain themselves. Only the first two can be skipped: their
/// result has an existing "unknown" rendering to fall back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlowRead {
    /// Branch ahead/behind counts against their upstreams (inside the snapshot).
    AheadBehind,
    /// Linked worktrees' working-tree status (inside the snapshot).
    Worktrees,
    /// The worktree size measurement (#633 inspection).
    WorktreeSize,
    /// Analyze's hotspot scan.
    Analyze,
    /// A large diff read off the UI thread (#495 text-first pipeline).
    Diff,
}

impl SlowRead {
    /// Tag for the `[kagi] busy: slow <op> after 2s` line.
    pub fn tag(self) -> &'static str {
        match self {
            SlowRead::AheadBehind => "ahead-behind",
            SlowRead::Worktrees => "worktrees",
            SlowRead::WorktreeSize => "worktree-size",
            SlowRead::Analyze => "analyze",
            SlowRead::Diff => "diff",
        }
    }

    /// Skip stops the read and shows its result as the existing "unknown":
    /// ahead/behind as `—`, the worktree size as "not measured". The others
    /// have no such rendering (a worktree without `wip` reads as clean), so
    /// they only explain.
    pub fn skippable(self) -> bool {
        matches!(self, SlowRead::AheadBehind | SlowRead::WorktreeSize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary is inclusive: two seconds exactly is already slow, a
    /// millisecond less is not.
    #[test]
    fn two_seconds_is_the_threshold() {
        assert!(!is_slow(Duration::ZERO));
        assert!(!is_slow(Duration::from_millis(1_999)));
        assert!(is_slow(Duration::from_secs(2)));
        assert!(is_slow(Duration::from_secs(30)));
    }
}
