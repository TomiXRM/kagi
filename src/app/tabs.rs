//! Tab lifecycle decisions. Pure index arithmetic, no view state.
//!
//! Editing the tab list and switching the active repository are two different
//! transitions (#488). Closing a background tab only renumbers the strip; only
//! closing the *active* tab activates another repository and re-initializes the
//! session. `#528` reuses the same decision when a completed remove deletes the
//! worktree a tab was pointing at.

/// What closing one tab leaves behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabClose {
    /// `closed` is out of range — nothing to do.
    Nothing,
    /// The last tab went away: no repository stays open (Welcome).
    Welcome,
    /// The active repository is untouched; its index shifts to this value.
    /// The caller MUST NOT reset per-repo state or re-arm anything (#488).
    Keep(usize),
    /// The active tab itself was closed: activate this index instead.
    Activate(usize),
}

/// Decide what to do after removing tab `closed` from `len` open tabs whose
/// active index is `active`.
pub fn close_tab(len: usize, closed: usize, active: usize) -> TabClose {
    if closed >= len {
        return TabClose::Nothing;
    }
    if len == 1 {
        return TabClose::Welcome;
    }
    // `len >= 2`, so `len - 2` is the last index after the removal.
    let active = active.min(len - 1);
    match closed.cmp(&active) {
        std::cmp::Ordering::Equal => TabClose::Activate(active.min(len - 2)),
        std::cmp::Ordering::Less => TabClose::Keep(active - 1),
        std::cmp::Ordering::Greater => TabClose::Keep(active),
    }
}

#[cfg(test)]
mod tests {
    use super::{close_tab, TabClose};

    #[test]
    fn out_of_range_and_last_tab() {
        assert_eq!(close_tab(0, 0, 0), TabClose::Nothing);
        assert_eq!(close_tab(2, 2, 0), TabClose::Nothing);
        assert_eq!(close_tab(1, 0, 0), TabClose::Welcome);
    }

    #[test]
    fn background_close_never_reactivates() {
        // Closing a tab before the active one only renumbers it.
        assert_eq!(close_tab(3, 0, 2), TabClose::Keep(1));
        assert_eq!(close_tab(3, 0, 1), TabClose::Keep(0));
        // Closing a tab after the active one leaves the index alone.
        assert_eq!(close_tab(3, 2, 0), TabClose::Keep(0));
        assert_eq!(close_tab(3, 2, 1), TabClose::Keep(1));
    }

    #[test]
    fn closing_the_active_tab_moves_to_a_neighbour() {
        assert_eq!(close_tab(2, 0, 0), TabClose::Activate(0));
        assert_eq!(close_tab(2, 1, 1), TabClose::Activate(0));
        assert_eq!(close_tab(3, 1, 1), TabClose::Activate(1));
        assert_eq!(close_tab(3, 2, 2), TabClose::Activate(1));
    }

    #[test]
    fn every_decision_stays_in_range() {
        for len in 1..6 {
            for closed in 0..len {
                for active in 0..len {
                    match close_tab(len, closed, active) {
                        TabClose::Nothing => panic!("in-range close reported Nothing"),
                        TabClose::Welcome => assert_eq!(len, 1),
                        TabClose::Keep(i) | TabClose::Activate(i) => assert!(i < len - 1),
                    }
                }
            }
        }
    }

    #[test]
    fn a_bogus_active_index_is_clamped() {
        assert_eq!(close_tab(2, 0, 9), TabClose::Keep(0));
    }
}
