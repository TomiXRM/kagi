//! Ghost connectors for squash-merged branches (ADR-0139).
//!
//! A squash merge replays a branch as one *new* commit, so the branch's own
//! commits never become ancestors of the target. Git's graph is drawn from
//! parent pointers, so the branch stays a dead-end leaf forever — correct, and
//! consistently confusing (user report). This module draws the missing link as
//! a dashed line from the squash commit down to the tip it replayed:
//!
//! ```text
//!   ●  squash: feat        ← the commit that carries the change
//!   │╌╌╌╮
//!   ●   ╎                    dashed = patch-id equivalence, not a parent
//!   ●   ╎
//!       ●  feat (tip)      ← the dead-end leaf
//! ```
//!
//! The edge injection is the same shape stashes already use (ADR-0088): a lane
//! plus `Pass` edges through the rows in between and a curve at each end, all
//! as a post-pass over built rows. The layout algorithm in `kagi-domain` is not
//! touched.

use std::collections::HashMap;

use gpui::{AppContext, Context};
use kagi::graph::{EdgeKind, GraphEdge};
use kagi_git::ops::SquashLink;
use kagi_git::CommitId;

use super::commit_list::CommitRow;
use super::KagiApp;

/// Marks an edge as a ghost connector rather than a real parent link.
///
/// It rides on `GraphEdge::color`, which the painter otherwise reduces
/// `% NUM_COLORS`. A sentinel keeps the whole feature inside the UI layer —
/// a ghost lane can share a column with a real branch line (that is the point:
/// the connector reuses the tip's own now-dead column), so "is this lane a
/// ghost" is not answerable per lane the way `stash_lanes` is.
pub const GHOST_COLOR: usize = usize::MAX;

/// Whether `lane`'s **top half** is already carrying a line at this row.
fn top_busy(row: &CommitRow, lane: usize) -> bool {
    row.edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::Pass | EdgeKind::IntoNode) && e.from_lane == lane)
}

/// Whether `lane`'s **bottom half** is already carrying a line at this row.
fn bottom_busy(row: &CommitRow, lane: usize) -> bool {
    row.edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::Pass | EdgeKind::OutOfNode) && e.to_lane == lane)
}

/// Whether a connector can run down `lane` from row `top` to row `bottom`
/// without crossing an existing line.
fn lane_free(rows: &[CommitRow], top: usize, bottom: usize, lane: usize) -> bool {
    if bottom_busy(&rows[top], lane) || top_busy(&rows[bottom], lane) {
        return false;
    }
    rows[top + 1..bottom]
        .iter()
        .all(|r| r.lane != lane && !top_busy(r, lane) && !bottom_busy(r, lane))
}

/// Inject one dashed connector per proven squash merge. Returns how many were
/// drawn (links whose two ends are not both in the loaded window are skipped).
///
/// Column choice matters more than it looks: a branch's own lane is dead above
/// its tip, and that is exactly the empty channel the connector wants, so it is
/// tried first and the graph does not get wider. Only when something else has
/// reclaimed that column does the connector take a new lane of its own.
pub fn inject_squash_edges(
    rows: &mut [CommitRow],
    links: &[SquashLink],
    index: &HashMap<CommitId, usize>,
) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let mut next_lane = rows[0].lane_count;
    let start_lane_count = next_lane;
    let mut drawn = 0usize;

    for link in links {
        let (Some(&top), Some(&bottom)) = (
            index.get(&CommitId(link.squash.clone())),
            index.get(&CommitId(link.tip.clone())),
        ) else {
            continue;
        };
        // The squash commit is created after the work it replays, so it sits
        // above the tip. Anything else is not a shape we can draw.
        if top >= bottom {
            continue;
        }
        let tip_lane = rows[bottom].lane;
        let lane = if lane_free(rows, top, bottom, tip_lane) {
            tip_lane
        } else {
            let l = next_lane;
            next_lane += 1;
            l
        };

        let squash_lane = rows[top].lane;
        rows[top].edges.push(GraphEdge {
            from_lane: squash_lane,
            to_lane: lane,
            kind: EdgeKind::OutOfNode,
            color: GHOST_COLOR,
        });
        for r in rows[top + 1..bottom].iter_mut() {
            r.edges.push(GraphEdge {
                from_lane: lane,
                to_lane: lane,
                kind: EdgeKind::Pass,
                color: GHOST_COLOR,
            });
        }
        rows[bottom].edges.push(GraphEdge {
            from_lane: lane,
            to_lane: tip_lane,
            kind: EdgeKind::IntoNode,
            color: GHOST_COLOR,
        });
        drawn += 1;
    }

    if next_lane != start_lane_count {
        for r in rows.iter_mut() {
            r.lane_count = next_lane;
        }
    }
    drawn
}

impl KagiApp {
    /// Scan for squash-merged branches in the background and draw their ghost
    /// connectors when the result lands.
    ///
    /// Off the snapshot path for the same reason Branch Cleanup is (ADR-0128):
    /// measured at ~600ms on an 1100-commit repo with 24 stale branches, which
    /// is not something to run on the UI thread after every git operation.
    pub fn start_squash_link_scan(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        self.squash_gen += 1;
        let my_gen = self.squash_gen;

        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&bg_path).and_then(|b| b.collect_squash_links())
        });

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                // Superseded: the repo changed, or a newer reload started one.
                let still_ours = app.squash_gen == my_gen
                    && app.repo_path.as_deref() == Some(repo_path.as_path());
                if !still_ours {
                    return;
                }
                let Ok(links) = result else {
                    return;
                };
                let index = app.active_view.commit_row_index.clone();
                let drawn = inject_squash_edges(&mut app.active_view.rows, &links, &index);
                klog!("squash-links: {} found, {} drawn", links.len(), drawn);
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ix: usize, lane: usize) -> CommitRow {
        let mut r = CommitRow::empty_for_test(CommitId(format!("{ix:040}")));
        r.lane = lane;
        r.lane_count = 2;
        r
    }

    fn link(tip: usize, squash: usize) -> SquashLink {
        SquashLink {
            branch: "feat".into(),
            tip: format!("{tip:040}"),
            squash: format!("{squash:040}"),
        }
    }

    /// The whole point: a connector spans every row between the two ends, so
    /// the line is continuous instead of two floating stubs.
    #[test]
    fn connects_squash_commit_down_to_the_tip() {
        // Row 0 = squash commit on lane 0, row 3 = the dead-end tip on lane 1.
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 0), row(3, 1)];
        let index: HashMap<CommitId, usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();

        assert_eq!(inject_squash_edges(&mut rows, &[link(3, 0)], &index), 1);

        let ghosts = |i: usize| -> Vec<GraphEdge> {
            rows[i]
                .edges
                .iter()
                .filter(|e| e.color == GHOST_COLOR)
                .cloned()
                .collect()
        };
        assert_eq!(ghosts(0).len(), 1);
        assert_eq!(ghosts(0)[0].kind, EdgeKind::OutOfNode);
        assert_eq!(
            ghosts(1).len(),
            1,
            "the rows in between must carry the line"
        );
        assert_eq!(ghosts(2)[0].kind, EdgeKind::Pass);
        assert_eq!(ghosts(3)[0].kind, EdgeKind::IntoNode);
        // The tip's own dead column was free, so the graph did not get wider.
        assert_eq!(rows[0].lane_count, 2);
    }

    /// A link pointing at a commit outside the loaded window draws nothing
    /// rather than half a line.
    #[test]
    fn skips_a_link_that_is_not_fully_loaded() {
        let mut rows = vec![row(0, 0), row(1, 1)];
        let index: HashMap<CommitId, usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        assert_eq!(inject_squash_edges(&mut rows, &[link(1, 99)], &index), 0);
        assert!(rows.iter().all(|r| r.edges.is_empty()));
    }
}
