//! One lane per pull request, rows in time order — the swimlane view's layout.
//!
//! Deliberately *not* [`crate::graph`]. That one solves lane assignment for a
//! commit DAG and requires a topologically-ordered slice; a set of open PRs is
//! a set of **disjoint** `merge-base..head` ranges with no edges between them,
//! and handing their concatenation to a topological layout is exactly the input
//! its own contract calls unspecified. The question here is simpler and has an
//! exact answer: each PR owns a lane, and the rows are the union of their
//! commits, newest first.

use crate::commit::{Commit, CommitId};

/// One row of the swimlane: a commit, and the lane (PR) it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneRow {
    /// Index into [`Swimlane::lanes`].
    pub lane: usize,
    pub commit: CommitId,
    pub subject: String,
    /// Committer time, seconds since the epoch — what the rows are ordered by.
    pub when: i64,
    /// True for the newest commit of its lane: the PR's head, which is where
    /// the lane's label belongs.
    pub is_tip: bool,
}

/// The laid-out swimlane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swimlane {
    /// PR numbers, one per lane, in the order the lanes were given.
    pub lanes: Vec<u64>,
    /// Rows, newest commit first.
    pub rows: Vec<LaneRow>,
}

impl Swimlane {
    /// Which lane a PR occupies, if it is in the view.
    pub fn lane_of(&self, pr: u64) -> Option<usize> {
        self.lanes.iter().position(|n| *n == pr)
    }
}

/// Lay out `lanes` — `(pr number, that PR's commits)` — as a swimlane.
///
/// Rows are ordered by committer time, newest first, so two PRs that were
/// pushed alternately read as interleaved work rather than as two blocks. Ties
/// keep the lane order given, so the view is stable across refreshes instead of
/// depending on a sort's accidents. A commit that appears in two PRs (one
/// stacked on the other) gets a row in each: it really is in both, and hiding
/// one would leave that lane with a gap it cannot explain.
pub fn lay_out(lanes: &[(u64, &[Commit])]) -> Swimlane {
    let mut rows: Vec<LaneRow> = Vec::with_capacity(lanes.iter().map(|(_, c)| c.len()).sum());
    for (lane, (_, commits)) in lanes.iter().enumerate() {
        // `commits` arrives newest-first (`merge-base..head`), so the first one
        // is the head — the tip of this lane.
        for (ix, commit) in commits.iter().enumerate() {
            rows.push(LaneRow {
                lane,
                commit: commit.id.clone(),
                subject: commit.summary.clone(),
                when: commit.committer.time,
                is_tip: ix == 0,
            });
        }
    }
    // Stable, so equal timestamps keep lane order (and within a lane, the order
    // the range gave).
    rows.sort_by(|a, b| b.when.cmp(&a.when));
    Swimlane {
        lanes: lanes.iter().map(|(pr, _)| *pr).collect(),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::Signature;

    fn commit(id: &str, subject: &str, when: i64) -> Commit {
        let who = Signature {
            name: "t".into(),
            email: "t@e".into(),
            time: when,
        };
        Commit {
            id: CommitId(id.into()),
            parents: Vec::new(),
            author: who.clone(),
            committer: who,
            summary: subject.into(),
            message: subject.into(),
        }
    }

    #[test]
    fn lanes_interleave_by_time_and_keep_their_pr() {
        let a = [commit("a2", "a newer", 200), commit("a1", "a older", 100)];
        let b = [commit("b2", "b newest", 300), commit("b1", "b oldest", 50)];
        let view = lay_out(&[(680, &a), (712, &b)]);

        assert_eq!(view.lanes, vec![680, 712]);
        assert_eq!(view.lane_of(712), Some(1));
        assert_eq!(view.lane_of(999), None);
        assert_eq!(
            view.rows
                .iter()
                .map(|r| (r.commit.0.as_str(), r.lane))
                .collect::<Vec<_>>(),
            vec![("b2", 1), ("a2", 0), ("a1", 0), ("b1", 1)],
            "newest first, each row on its own PR's lane"
        );
    }

    #[test]
    fn only_the_head_of_each_lane_is_a_tip() {
        let a = [commit("a2", "head", 200), commit("a1", "older", 100)];
        let b = [commit("b1", "lone", 150)];
        let view = lay_out(&[(1, &a), (2, &b)]);
        let tips: Vec<&str> = view
            .rows
            .iter()
            .filter(|r| r.is_tip)
            .map(|r| r.commit.0.as_str())
            .collect();
        assert_eq!(tips, vec!["a2", "b1"], "one tip per lane, and only one");
    }

    #[test]
    fn equal_timestamps_keep_the_lane_order_they_were_given() {
        let a = [commit("a", "a", 100)];
        let b = [commit("b", "b", 100)];
        let c = [commit("c", "c", 100)];
        let view = lay_out(&[(3, &a), (1, &b), (2, &c)]);
        assert_eq!(
            view.rows.iter().map(|r| r.lane).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "a refresh must not shuffle rows that share a second"
        );
    }

    #[test]
    fn a_commit_in_two_prs_gets_a_row_in_each() {
        // #712 stacked on #680: the shared commit belongs to both ranges.
        let shared = commit("s1", "shared", 100);
        let base = [shared.clone()];
        let stacked = [commit("s2", "on top", 120), shared];
        let view = lay_out(&[(680, &base), (712, &stacked)]);
        let shared_lanes: Vec<usize> = view
            .rows
            .iter()
            .filter(|r| r.commit.0 == "s1")
            .map(|r| r.lane)
            .collect();
        assert_eq!(shared_lanes, vec![0, 1]);
    }

    #[test]
    fn nothing_open_is_an_empty_view_not_a_panic() {
        let view = lay_out(&[]);
        assert!(view.rows.is_empty() && view.lanes.is_empty());
        let empty: [Commit; 0] = [];
        let view = lay_out(&[(1, &empty)]);
        assert_eq!(view.lanes, vec![1]);
        assert!(view.rows.is_empty());
    }
}
