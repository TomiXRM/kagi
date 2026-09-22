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
//! HEAD and an `IntoNode` at HEAD. `kagi-domain`'s layout is untouched; the
//! column choice itself is [`crate::graph::wip_anchor`] — pure, and sharing
//! the half-row occupancy rules with the squash connector rather than copying
//! them.
//!
//! The one thing squash's `GHOST_COLOR` could not express is *which* worktree a
//! line belongs to — it is a single fixed grey. So the sentinel becomes a
//! range: `WIP_GHOST_BASE + <lane colour index>` means "dashed, in that lane
//! colour", which is what makes two connectors tellable apart.
//!
//! #767: that colour is **HEAD's** lane colour, not the worktree's ordinal,
//! and worktrees sharing a HEAD share the whole anchor — one column, one
//! colour, one trace down to the single node they all sit on. Drawing k
//! identical lines in k different worktree colours said "k targets" about one.

use std::collections::HashMap;

use kagi_git::{CommitId, Head, RepoSnapshot};

use crate::graph::{self, EdgeKind, GraphEdge, RowOccupancy, WipAnchor};

use super::commit_list::CommitRow;
use super::graph_squash::GHOST_COLOR;

/// Start of the WIP-ghost sentinel range on `GraphEdge::color`.
///
/// `color = WIP_GHOST_BASE + idx` means "draw this dashed, in `lane_color(idx)`".
/// It rides on `color` for the same reason the squash sentinel does: a
/// connector deliberately shares a column with real branch lines, so "is this
/// dashed" is a property of the edge, not of the lane. `usize::MAX` itself
/// stays `GHOST_COLOR`, so the range stops one short of it.
pub const WIP_GHOST_BASE: usize = usize::MAX - 64;

/// Highest colour index the sentinel range can carry (`WIP_GHOST_BASE + 63`);
/// 64 would land on `GHOST_COLOR`. HEAD colours are bounded by
/// `graph::NUM_COLORS` (8), so every graph colour fits without clamping.
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

/// Which WIP row a connector belongs to (#476 slice 2).
///
/// The lanes used to be handed back to `render_body` **by position**, which
/// only held while both lists were derived from the same snapshot. Committing
/// from a linked worktree's panel makes that worktree clean, so its row leaves
/// `render_body`'s list while the snapshot-built `wip_lanes` still carries its
/// entry — and every row below it silently inherits the wrong lane and colour.
/// Keying by target instead makes the mismatch a `None` for the row that is
/// gone, and leaves every other row's lane alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WipTarget {
    /// The open repository's own working tree.
    Current,
    /// The linked worktree at that index in `snap.worktrees`.
    Worktree(usize),
}

/// The anchor recorded for `target` — the column its connector took and the
/// colour it is drawn in — or `None` when this view has no connector for it
/// (its row was not drawn, or the snapshot predates its row).
pub fn wip_anchor_for(
    lanes: &[(WipTarget, Option<WipAnchor>)],
    target: WipTarget,
) -> Option<WipAnchor> {
    lanes
        .iter()
        .find(|(t, _)| *t == target)
        .and_then(|(_, anchor)| *anchor)
}

/// The lane recorded for `target`, or `None` when no connector was drawn.
pub fn wip_lane(lanes: &[(WipTarget, Option<WipAnchor>)], target: WipTarget) -> Option<usize> {
    wip_anchor_for(lanes, target).map(|a| a.lane)
}

/// The connector anchor for each WIP row about to be drawn, in row order.
///
/// This is the whole row ↔ anchor join, in one place so `render_body` and its
/// tests agree: each row is looked up **by its own target**, so a `lanes` map
/// that is a frame out of date (built before the row list changed — the commit
/// panel's write ops refresh a worktree's WIP row in place, the re-snapshot
/// lands a frame later) costs only the rows that are actually missing from it.
/// By position, one vanished row shifted every row below it onto a stranger's
/// lane and colour.
pub fn lanes_for_rows(
    lanes: &[(WipTarget, Option<WipAnchor>)],
    rows: &[WipTarget],
) -> Vec<Option<WipAnchor>> {
    rows.iter().map(|t| wip_anchor_for(lanes, *t)).collect()
}

/// The WIP rows `render_body` draws, in order, as `(target, HEAD)`.
///
/// Kept next to the injector because the two must agree: the anchors this
/// module returns are looked up by `target`. The order mirrors `render_body`:
/// the open repo's own row first (when its working tree is dirty), then every
/// *other* dirty worktree in `snap.worktrees` order.
///
/// HEAD is always an object id — `Head::Attached` carries the branch tip's own
/// sha, and a detached HEAD is already the commit — so a detached worktree
/// anchors exactly like an attached one. An unborn HEAD has no commit at all
/// and yields `None` here, which the injector turns into "no connector".
pub fn wip_targets(snap: &RepoSnapshot) -> Vec<(WipTarget, Option<CommitId>)> {
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
        out.push((WipTarget::Current, head));
    }
    for (idx, wt) in snap.worktrees.iter().enumerate() {
        if wt.is_current || !wt.wip.is_some_and(|w| w.is_dirty()) {
            continue;
        }
        out.push((WipTarget::Worktree(idx), wt.head.clone()));
    }
    out
}

/// Inject the dashed connectors and return the anchor each WIP row landed on,
/// **keyed by its target** — `None` where no line was drawn (unborn HEAD, or a
/// HEAD outside the loaded window, the same "half a line is worse than none"
/// rule stashes use for `connected == false`).
///
/// One connector per *distinct* HEAD row, not per row (#767): worktrees parked
/// on the same commit all map to the same anchor, so the graph carries one
/// trace in HEAD's colour instead of k lines stacked in one column. The lane
/// choice itself is [`crate::graph::wip_anchor`]; distinct HEADs still never
/// collide, because each injection's own `Pass` edges make its column busy for
/// the next one.
pub fn inject_wip_edges(
    rows: &mut [CommitRow],
    targets: &[(WipTarget, Option<CommitId>)],
    index: &HashMap<CommitId, usize>,
) -> Vec<(WipTarget, Option<WipAnchor>)> {
    let mut anchors: Vec<(WipTarget, Option<WipAnchor>)> =
        targets.iter().map(|(t, _)| (*t, None)).collect();
    if rows.is_empty() {
        return anchors;
    }
    let mut next_lane = rows[0].lane_count;
    let start_lane_count = next_lane;
    // HEAD row → the anchor already drawn for it, in injection order. A `Vec`
    // because k is the number of dirty worktrees: single digits, always.
    let mut drawn: Vec<(usize, WipAnchor)> = Vec::new();

    for (i, (_, head)) in targets.iter().enumerate() {
        // `rows` and `index` are reconciled across an async boundary
        // elsewhere, so bound-check rather than trusting that they agree.
        let bottom = head.as_ref().and_then(|h| index.get(h)).copied();
        let Some(bottom) = bottom.filter(|&b| b < rows.len()) else {
            continue;
        };
        if let Some((_, shared)) = drawn.iter().find(|(b, _)| *b == bottom) {
            anchors[i].1 = Some(*shared);
            continue;
        }
        // Project borrowed rows lazily: no per-worktree occupancy allocation.
        let anchor = graph::wip_anchor(
            rows[..=bottom].iter().map(|r| RowOccupancy {
                lane: r.lane,
                color: r.node_color,
                edges: &r.edges,
            }),
            Some(bottom),
            next_lane,
        );
        let Some(anchor) = anchor else { continue };
        // HEAD's own column is always below `lane_count`, so equality here can
        // only mean the selector took the fresh column that was offered.
        if anchor.lane == next_lane {
            next_lane += 1;
        }
        let color = wip_color(anchor.color);
        let head_lane = rows[bottom].lane;

        for r in rows[..bottom].iter_mut() {
            r.edges.push(GraphEdge {
                from_lane: anchor.lane,
                to_lane: anchor.lane,
                kind: EdgeKind::Pass,
                color,
            });
        }
        rows[bottom].edges.push(GraphEdge {
            from_lane: anchor.lane,
            to_lane: head_lane,
            kind: EdgeKind::IntoNode,
            color,
        });
        drawn.push((bottom, anchor));
        anchors[i].1 = Some(anchor);
    }

    if next_lane != start_lane_count {
        for r in rows.iter_mut() {
            r.lane_count = next_lane;
        }
    }
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blank row on `lane`, whose node carries lane colour `color` — the
    /// colour a connector aiming at this row is drawn in (#767).
    fn row(ix: usize, lane: usize, color: usize) -> CommitRow {
        let mut r = CommitRow::empty_for_test(CommitId(format!("{ix:040}")));
        r.lane = lane;
        r.node_color = color;
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
        let mut rows = vec![row(0, 0, 4), row(1, 0, 4), row(2, 0, 4), row(3, 0, 4)];
        for r in rows[..3].iter_mut() {
            r.edges.push(GraphEdge {
                from_lane: 0,
                to_lane: 0,
                kind: EdgeKind::Pass,
                color: 0,
            });
        }
        let ix = index(&rows);

        let anchors = inject_wip_edges(&mut rows, &[(WipTarget::Current, Some(id(3)))], &ix);
        assert_eq!(
            anchors,
            vec![(WipTarget::Current, Some(WipAnchor { lane: 2, color: 4 }))],
            "lane 0 is busy → a fresh lane (2), in HEAD's own colour"
        );

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
        // colour of the branch line it lands on.
        assert_eq!(wip_color_index(into[0].color), Some(4));
    }

    /// HEAD's own column is empty above it (nothing branched off it), so the
    /// connector reuses it and the graph does not get wider.
    #[test]
    fn reuses_heads_lane_when_free() {
        let mut rows = vec![row(0, 0, 0), row(1, 0, 0), row(2, 1, 6)];
        let ix = index(&rows);
        let anchors = inject_wip_edges(&mut rows, &[(WipTarget::Worktree(3), Some(id(2)))], &ix);
        assert_eq!(
            anchors,
            vec![(
                WipTarget::Worktree(3),
                Some(WipAnchor { lane: 1, color: 6 })
            )]
        );
        assert!(
            rows.iter().all(|r| r.lane_count == 2),
            "reusing a column must not widen the graph"
        );
        assert_eq!(wip_color_index(ghosts(&rows[2])[0].color), Some(6));
    }

    /// HEAD outside the loaded window draws nothing rather than half a line,
    /// and an unborn HEAD has nothing to draw to at all.
    #[test]
    fn draws_nothing_when_head_is_not_loaded() {
        let mut rows = vec![row(0, 0, 0), row(1, 0, 0)];
        let ix = index(&rows);
        let anchors = inject_wip_edges(
            &mut rows,
            &[
                (WipTarget::Current, Some(id(99))),
                (WipTarget::Worktree(1), None),
            ],
            &ix,
        );
        assert_eq!(
            anchors,
            vec![(WipTarget::Current, None), (WipTarget::Worktree(1), None)]
        );
        assert!(rows.iter().all(|r| r.edges.is_empty()));
    }

    /// #767: two dirty worktrees parked on the *same* HEAD describe one
    /// target, so they share one anchor — one column, one colour, one trace.
    /// They used to take a lane each and stack two dashed lines of different
    /// colours on top of the same commit.
    #[test]
    fn worktrees_on_one_head_share_a_single_anchor() {
        let mut rows = vec![row(0, 0, 0), row(1, 0, 0), row(2, 1, 6)];
        let ix = index(&rows);
        let anchors = inject_wip_edges(
            &mut rows,
            &[
                (WipTarget::Current, Some(id(2))),
                (WipTarget::Worktree(1), Some(id(2))),
            ],
            &ix,
        );
        let shared = WipAnchor { lane: 1, color: 6 };
        assert_eq!(
            anchors,
            vec![
                (WipTarget::Current, Some(shared)),
                (WipTarget::Worktree(1), Some(shared)),
            ],
            "both rows anchor on HEAD's own column, in HEAD's colour"
        );
        // Exactly one trace, not one per worktree.
        assert_eq!(ghosts(&rows[0]).len(), 1);
        assert_eq!(ghosts(&rows[1]).len(), 1);
        let into = ghosts(&rows[2]);
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].kind, EdgeKind::IntoNode);
        assert_eq!(wip_color_index(into[0].color), Some(6));
        assert!(
            rows.iter().all(|r| r.lane_count == 2),
            "a shared anchor must not widen the graph"
        );
    }

    /// Worktrees on *different* HEADs still never share a column, even when
    /// both HEADs sit in the same one: the first connector's own `Pass` edges
    /// make that column busy, so the second is pushed out to a fresh lane and
    /// keeps its own HEAD's colour.
    #[test]
    fn distinct_heads_never_share_a_column() {
        // Lane 1 carries two separate lines: row 1's, and (after it ends) row
        // 3's. Both are somebody's HEAD.
        let mut rows = vec![row(0, 0, 2), row(1, 1, 5), row(2, 0, 2), row(3, 1, 7)];
        let ix = index(&rows);
        let anchors = inject_wip_edges(
            &mut rows,
            &[
                (WipTarget::Current, Some(id(1))),
                (WipTarget::Worktree(1), Some(id(3))),
            ],
            &ix,
        );
        assert_eq!(
            anchors,
            vec![
                (WipTarget::Current, Some(WipAnchor { lane: 1, color: 5 })),
                (
                    WipTarget::Worktree(1),
                    Some(WipAnchor { lane: 2, color: 7 })
                ),
            ],
            "the shallower HEAD keeps lane 1; the deeper one is pushed out"
        );
        assert!(
            rows.iter().all(|r| r.lane_count == 3),
            "the fresh column widens the graph"
        );
    }

    /// #476 slice 2: committing from a linked worktree's panel makes that
    /// worktree clean, so its WIP row leaves `render_body`'s list while a
    /// snapshot-built lane map still carries its entry. Keyed by target, the
    /// row that is gone yields `None` and **every other row keeps its lane** —
    /// positionally, each row below would have inherited its neighbour's.
    #[test]
    fn a_missing_target_does_not_shift_the_other_lanes() {
        let anchor = |lane, color| Some(WipAnchor { lane, color });
        let lanes = vec![
            (WipTarget::Current, anchor(4, 1)),
            (WipTarget::Worktree(1), anchor(5, 2)),
            (WipTarget::Worktree(2), anchor(6, 3)),
        ];
        assert_eq!(wip_lane(&lanes, WipTarget::Current), Some(4));
        assert_eq!(wip_anchor_for(&lanes, WipTarget::Worktree(2)), anchor(6, 3));
        // Worktree 1 just committed: it is no longer a row, and nothing else
        // moved — worktree 2 still reads lane 6, not 5.
        let lanes: Vec<_> = lanes
            .into_iter()
            .filter(|(t, _)| *t != WipTarget::Worktree(1))
            .collect();
        assert_eq!(wip_lane(&lanes, WipTarget::Worktree(1)), None);
        assert_eq!(wip_lane(&lanes, WipTarget::Current), Some(4));
        assert_eq!(wip_anchor_for(&lanes, WipTarget::Worktree(2)), anchor(6, 3));
        // A target the map never had is `None`, not a panic or a neighbour.
        assert_eq!(wip_lane(&lanes, WipTarget::Worktree(9)), None);
        // A drawn-but-laneless row stays `None` too.
        assert_eq!(
            wip_lane(&[(WipTarget::Current, None)], WipTarget::Current),
            None
        );
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
