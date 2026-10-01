//! Which reflog lines belong to an Operation Log entry (#334 slice 1, ADR-0214).
//!
//! An oplog entry records `before`/`after` as display text, not object ids, so
//! the reflog lines it caused can only be found by **time**: the entry is
//! stamped once its outcome is known (after the refs moved), and the previous
//! entry of the same worktree bounds the window from below. Everything here is
//! display evidence. A line marked [`Attribution::Ambiguous`] shares its second
//! with another operation of the same worktree and must never become the
//! premise of a restore or revert (ADR-0214 §3); the exact pairing is the OIDs
//! a future entry will carry.

/// One reflog line, as read from `logs/HEAD` or `logs/refs/heads/*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogLine {
    /// `HEAD` or the full branch ref (`refs/heads/main`).
    pub refname: String,
    pub old: String,
    pub new: String,
    pub message: String,
    /// Unix seconds of the reflog signature.
    pub time: i64,
}

/// How far back the window reaches when the worktree's previous operation is
/// not in the loaded log: a write is recorded within seconds of its ref moves.
pub const UNBOUNDED_LOOKBACK_SECS: i64 = 60;

/// The `(after, until]` span of reflog time that belongs to one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReflogWindow {
    /// Exclusive lower bound: the previous same-worktree entry's second.
    pub after: i64,
    /// Inclusive upper bound: this entry's own second.
    pub until: i64,
    /// Another entry of the same worktree was recorded in `until`'s second, so
    /// a line in that second cannot be told apart.
    pub shared_second: bool,
    /// No earlier entry of this worktree was loaded; `after` is the lookback.
    pub open_start: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribution {
    /// Inside the window, in a second no other operation of the worktree used.
    Within,
    /// In the entry's second, which another operation of the worktree shares.
    Ambiguous,
}

impl ReflogWindow {
    /// `older` / `newer`: timestamps of the neighbouring entries of the same
    /// worktree (`None` when not loaded).
    pub fn for_entry(timestamp: i64, older: Option<i64>, newer: Option<i64>) -> Self {
        let after = match older {
            Some(o) if o < timestamp => o,
            // Same second (or a clock step back): only that second is in play,
            // and it is shared.
            Some(_) => timestamp - 1,
            None => timestamp - UNBOUNDED_LOOKBACK_SECS,
        };
        Self {
            after,
            until: timestamp,
            shared_second: older.is_some_and(|o| o >= timestamp)
                || newer.is_some_and(|n| n == timestamp),
            open_start: older.is_none(),
        }
    }

    pub fn classify(&self, time: i64) -> Option<Attribution> {
        if time <= self.after || time > self.until {
            None
        } else if time == self.until && self.shared_second {
            Some(Attribution::Ambiguous)
        } else {
            Some(Attribution::Within)
        }
    }

    /// The lines inside the window, oldest first, each with its attribution.
    pub fn attribute(&self, lines: Vec<ReflogLine>) -> Vec<(ReflogLine, Attribution)> {
        let mut kept: Vec<_> = lines
            .into_iter()
            .filter_map(|line| self.classify(line.time).map(|a| (line, a)))
            .collect();
        kept.sort_by(|(a, _), (b, _)| a.time.cmp(&b.time).then(a.refname.cmp(&b.refname)));
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(refname: &str, time: i64) -> ReflogLine {
        ReflogLine {
            refname: refname.into(),
            old: "0".repeat(40),
            new: "1".repeat(40),
            message: format!("{refname}@{time}"),
            time,
        }
    }

    #[test]
    fn a_line_belongs_to_the_first_entry_recorded_at_or_after_it() {
        // Entries of one worktree at 100 and 105; lines at 100..=105.
        let first = ReflogWindow::for_entry(100, Some(90), Some(105));
        let second = ReflogWindow::for_entry(105, Some(100), None);
        for t in 91..=105 {
            let owners = [first.classify(t), second.classify(t)]
                .iter()
                .filter(|a| a.is_some())
                .count();
            assert_eq!(owners, 1, "line at {t} must have exactly one owner");
        }
        assert_eq!(first.classify(100), Some(Attribution::Within));
        assert_eq!(first.classify(101), None, "after its record: the next op's");
        assert_eq!(second.classify(100), None, "the previous op's own second");
        assert_eq!(second.classify(101), Some(Attribution::Within));
        assert_eq!(second.classify(106), None, "after the entry was recorded");
    }

    #[test]
    fn a_second_shared_by_two_entries_is_ambiguous_for_both_never_assigned() {
        let first = ReflogWindow::for_entry(200, Some(150), Some(200));
        let second = ReflogWindow::for_entry(200, Some(200), None);
        assert_eq!(first.classify(200), Some(Attribution::Ambiguous));
        assert_eq!(second.classify(200), Some(Attribution::Ambiguous));
        assert_eq!(first.classify(199), Some(Attribution::Within));
        assert_eq!(
            second.classify(199),
            None,
            "the earlier second is not shared"
        );
    }

    #[test]
    fn without_a_loaded_predecessor_the_window_is_bounded_and_says_so() {
        let w = ReflogWindow::for_entry(1_000, None, None);
        assert!(w.open_start);
        assert_eq!(w.classify(1_000 - UNBOUNDED_LOOKBACK_SECS), None);
        assert_eq!(
            w.classify(1_000 - UNBOUNDED_LOOKBACK_SECS + 1),
            Some(Attribution::Within)
        );
    }

    #[test]
    fn attribute_keeps_only_the_window_oldest_first() {
        let w = ReflogWindow::for_entry(50, Some(40), None);
        let kept = w.attribute(vec![
            line("HEAD", 50),
            line("refs/heads/a", 39),
            line("refs/heads/a", 45),
            line("HEAD", 51),
        ]);
        let times: Vec<i64> = kept.iter().map(|(l, _)| l.time).collect();
        assert_eq!(times, vec![45, 50]);
    }
}
