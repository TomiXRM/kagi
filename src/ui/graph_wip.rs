//! Dashed connectors from each WIP row down to that worktree's HEAD (#472).
//!
//! The WIP rows sit above the graph, so when `origin` is ahead — HEAD buried a
//! dozen rows down — nothing on screen says *which* commit the next commit will
//! sit on top of. With several dirty worktrees (Model A+ draws one WIP row per
//! worktree) it is worse: k rows, k different HEADs, no way to pair them.
//!
//! ```text
//!   ◦╌╌╌╌╌╮        WIP: feat        ← dot, no avatar (nothing committed yet)
//!   ●     ╎  origin tip
//!   ●     ╎
//!   ●╌╌╌╌╌╯  HEAD                   ← solid curve into the node
//! ```
//!
//! Mechanically this is the squash ghost connector (ADR-0139,
//! `graph_squash.rs`) with a different pair of endpoints: a post-pass over the
//! built `CommitRow`s that picks a lane, pushes `Pass` through every row above
//! HEAD and an `IntoNode` at HEAD. `kagi-domain`'s layout is untouched, and the
//! lane helpers (`top_busy` / `bottom_busy`) are shared rather than copied.
//!
//! The one thing squash's `GHOST_COLOR` could not express is *which* worktree a
//! line belongs to — it is a single fixed grey. So the sentinel becomes a
//! range: `WIP_GHOST_BASE + <lane colour index>` means "dashed, in that lane
//! colour", which is what makes two connectors tellable apart.

use std::collections::HashMap;

use kagi_git::{CommitId, Head, RepoSnapshot};

use crate::graph::{EdgeKind, GraphEdge};

use super::commit_list::CommitRow;
use super::graph_squash::{bottom_busy, top_busy, GHOST_COLOR};

/// Start of the WIP-ghost sentinel range on `GraphEdge::color`.
///
/// `color = WIP_GHOST_BASE + idx` means "draw this dashed, in `lane_color(idx)`".
/// It rides on `color` for the same reason the squash sentinel does: a
/// connector deliberately shares a column with real branch lines, so "is this
/// dashed" is a property of the edge, not of the lane. `usize::MAX` itself
/// stays `GHOST_COLOR`, so the range stops one short of it.
pub const WIP_GHOST_BASE: usize = usize::MAX - 64;

/// Highest colour index the sentinel range can carry (`WIP_GHOST_BASE + 63`);
/// 64 would land on `GHOST_COLOR`. Lane colours cycle every 6, so clamping a
/// 64th worktree here only reuses a colour it was going to share anyway.
const MAX_COLOR_IDX: usize = 63;

/// Encode a lane colour index as a WIP-ghost edge colour.
#[inline]
pub fn wip_color(color_idx: usize) -> usize {
    WIP_GHOST_BASE + color_idx.min(MAX_COLOR_IDX)
}

/// Decode a WIP-ghost edge colour back to its lane colour index, or `None` when
/// this is an ordinary (or squash-ghost) edge.
#[inline]
pub fn wip_color_index(color: usize) -> Option<usize> {
    (color >= WIP_GHOST_BASE && color != GHOST_COLOR).then(|| color - WIP_GHOST_BASE)
}

/// Whether a connector can run down `lane` from above row 0 to row `bottom`
/// without crossing an existing line.
///
/// Unlike `graph_squash::lane_free` the line enters from *outside* the graph
/// (the WIP row), so row 0's top half has to be free too.
fn lane_free_to(rows: &[CommitRow], bottom: usize, lane: usize) -> bool {
    !top_busy(&rows[bottom], lane)
        && rows[..bottom]
            .iter()
            .all(|r| r.lane != lane && !top_busy(r, lane) && !bottom_busy(r, lane))
}

/// The WIP rows `render_body` draws, in order, as `(lane colour index, HEAD)`.
///
/// Kept next to the injector because the two must agree row-for-row: the lanes
/// this module returns are handed back to the WIP rows by position. The order
/// mirrors `render_body`: the open repo's own row first (when its working tree
/// is dirty), then every *other* dirty worktree in `snap.worktrees` order.
pub fn wip_targets(snap: &RepoSnapshot) -> Vec<(usize, Option<CommitId>)> {
    let mut out = Vec::new();
    if snap.status.is_dirty() {
        // The open repo's row is driven by the live status, not by a worktree
        // entry (path canonicalization can fail to flag one `is_current`), so
        // its target comes from the snapshot's own HEAD.
        let head = match &snap.head {
            Head::Attached { target, .. } | Head::Detached { target } => {
                Some(CommitId(target.clone()))
            }
            Head::Unborn { .. } => None,
        };
        let cur = snap.worktrees.iter().position(|w| w.is_current);
        out.push((cur.unwrap_or(0), head));
    }
    for (idx, wt) in snap.worktrees.iter().enumerate() {
        if wt.is_current || !wt.wip.is_some_and(|w| w.is_dirty()) {
            continue;
        }
        out.push((idx, wt.head.clone()));
    }
    out
}

/// Inject one dashed connector per WIP row. Returns the lane each connector
/// took, positionally aligned with `targets` — `None` where no line was drawn
/// (unborn HEAD, or a HEAD outside the loaded window, the same "half a line is
/// worse than none" rule stashes use for `connected == false`).
///
/// Lane choice follows the squash precedent: reuse HEAD's own column when it is
/// free all the way up, otherwise take a fresh one. When `origin` is ahead that
/// column is occupied by HEAD's descendants, so a fresh lane is the normal
/// outcome — and the right one, since the connector must not overdraw them.
/// Connectors never collide with each other because each one's `Pass` edges
/// make its lane busy for the next.
pub fn inject_wip_edges(
    rows: &mut [CommitRow],
    targets: &[(usize, Option<CommitId>)],
    index: &HashMap<CommitId, usize>,
) -> Vec<Option<usize>> {
    let mut lanes: Vec<Option<usize>> = vec![None; targets.len()];
    if rows.is_empty() {
        return lanes;
    }
    let mut next_lane = rows[0].lane_count;
    let start_lane_count = next_lane;

    for (slot, (color_idx, head)) in lanes.iter_mut().zip(targets) {
        // `rows` and `index` are reconciled across an async boundary elsewhere,
        // so bound-check rather than trusting they agree.
        let Some(&bottom) = head.as_ref().and_then(|h| index.get(h)) else {
            continue;
        };
        if bottom >= rows.len() {
            continue;
        }
        let head_lane = rows[bottom].lane;
        let lane = if lane_free_to(rows, bottom, head_lane) {
            head_lane
        } else {
            let l = next_lane;
            next_lane += 1;
            l
        };
        let color = wip_color(*color_idx);

        for r in rows[..bottom].iter_mut() {
            r.edges.push(GraphEdge {
                from_lane: lane,
                to_lane: lane,
                kind: EdgeKind::Pass,
                color,
            });
        }
        rows[bottom].edges.push(GraphEdge {
            from_lane: lane,
            to_lane: head_lane,
            kind: EdgeKind::IntoNode,
            color,
        });
        *slot = Some(lane);
    }

    if next_lane != start_lane_count {
        for r in rows.iter_mut() {
            r.lane_count = next_lane;
        }
    }
    lanes
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

    fn index(rows: &[CommitRow]) -> HashMap<CommitId, usize> {
        rows.iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect()
    }

    fn id(ix: usize) -> CommitId {
        CommitId(format!("{ix:040}"))
    }

    /// The ghost edges of one row, in injection order.
    fn ghosts(row: &CommitRow) -> Vec<&GraphEdge> {
        row.edges
            .iter()
            .filter(|e| wip_color_index(e.color).is_some())
            .collect()
    }

    /// A branch line already runs down HEAD's column above it (origin ahead),
    /// so the connector must take a lane of its own rather than overdraw it.
    #[test]
    fn takes_a_fresh_lane_when_heads_column_is_busy() {
        // Rows 0..3 are one straight line on lane 0; HEAD is row 3.
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 0), row(3, 0)];
        for r in rows[..3].iter_mut() {
            r.edges.push(GraphEdge {
                from_lane: 0,
                to_lane: 0,
                kind: EdgeKind::Pass,
                color: 0,
            });
        }
        let ix = index(&rows);

        let lanes = inject_wip_edges(&mut rows, &[(0, Some(id(3)))], &ix);
        assert_eq!(lanes, vec![Some(2)], "lane 0 is busy → a fresh lane (2)");

        // One Pass per row above HEAD, and the curve into HEAD's own node.
        assert_eq!(ghosts(&rows[0]).len(), 1);
        assert_eq!(ghosts(&rows[1])[0].kind, EdgeKind::Pass);
        assert_eq!(ghosts(&rows[2])[0].kind, EdgeKind::Pass);
        let into = ghosts(&rows[3]);
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].kind, EdgeKind::IntoNode);
        assert_eq!(into[0].from_lane, 2);
        assert_eq!(into[0].to_lane, 0, "the curve lands on HEAD's node");
        assert_eq!(
            (0..3).filter(|&i| !ghosts(&rows[i]).is_empty()).count(),
            3,
            "Pass count must equal HEAD's row index"
        );
        // A fresh lane widens the graph — every row carries the new count.
        assert!(rows.iter().all(|r| r.lane_count == 3));
        // The colour index rides the sentinel, so the line is drawn in the
        // worktree's own lane colour.
        assert_eq!(wip_color_index(into[0].color), Some(0));
    }

    /// HEAD's own column is empty above it (nothing branched off it), so the
    /// connector reuses it and the graph does not get wider.
    #[test]
    fn reuses_heads_lane_when_free() {
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 1)];
        let ix = index(&rows);
        let lanes = inject_wip_edges(&mut rows, &[(3, Some(id(2)))], &ix);
        assert_eq!(lanes, vec![Some(1)]);
        assert!(
            rows.iter().all(|r| r.lane_count == 2),
            "reusing a column must not widen the graph"
        );
        assert_eq!(wip_color_index(ghosts(&rows[2])[0].color), Some(3));
    }

    /// HEAD outside the loaded window draws nothing rather than half a line.
    #[test]
    fn draws_nothing_when_head_is_not_loaded() {
        let mut rows = vec![row(0, 0), row(1, 0)];
        let ix = index(&rows);
        let lanes = inject_wip_edges(&mut rows, &[(0, Some(id(99))), (1, None)], &ix);
        assert_eq!(lanes, vec![None, None]);
        assert!(rows.iter().all(|r| r.edges.is_empty()));
    }

    /// Two dirty worktrees pointing at the *same* HEAD still get two distinct
    /// lanes — otherwise the second line would be drawn straight over the first
    /// and the colours would be unreadable.
    #[test]
    fn two_wip_rows_never_share_a_lane() {
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 1)];
        let ix = index(&rows);
        let lanes = inject_wip_edges(&mut rows, &[(0, Some(id(2))), (1, Some(id(2)))], &ix);
        assert_eq!(lanes.len(), 2);
        let (a, b) = (lanes[0].unwrap(), lanes[1].unwrap());
        assert_ne!(a, b, "two connectors must not share a column");
        // Colour indices stay tied to their worktree, not to the lane.
        let colors: Vec<Option<usize>> = ghosts(&rows[2])
            .iter()
            .map(|e| wip_color_index(e.color))
            .collect();
        assert_eq!(colors, vec![Some(0), Some(1)]);
    }

    /// The sentinel range must not be mistaken for a real colour index, and
    /// must not swallow the squash ghost.
    #[test]
    fn sentinel_range_is_distinguishable() {
        assert_eq!(wip_color_index(0), None);
        assert_eq!(wip_color_index(7), None);
        assert_eq!(wip_color_index(GHOST_COLOR), None);
        assert_eq!(wip_color_index(wip_color(5)), Some(5));
        // Clamped, never colliding with GHOST_COLOR.
        assert_ne!(wip_color(999), GHOST_COLOR);
    }
}
