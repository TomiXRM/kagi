//! Dashed connectors from each WIP row down to that worktree's HEAD (#472).
//!
//! The WIP rows sit above the graph, so when `origin` is ahead — HEAD buried a
//! dozen rows down — nothing on screen says *which* commit the next commit will
//! sit on top of. With several dirty worktrees (Model A+ draws one WIP row per
//! worktree) it is worse: k rows, k different HEADs, no way to pair them.
//!
//! WIP anchors sit directly above HEAD in its existing column. Their dashed
//! connectors pass behind intervening history rather than reserving another
//! column. `kagi-domain` owns that choice; this post-pass adds the annotation
//! edges without changing the real graph's lane count.
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
/// Worktrees sharing an anchor use one trace down to their deepest HEAD.
/// This includes a shared HEAD and distinct HEADs that reuse the same lane
/// and colour. Different colours remain distinct annotations.
pub fn inject_wip_edges(
    rows: &mut [CommitRow],
    targets: &[(WipTarget, Option<CommitId>)],
    index: &HashMap<CommitId, usize>,
) -> Vec<(WipTarget, Option<WipAnchor>)> {
    let mut anchors = Vec::with_capacity(targets.len());
    let mut drawn: Vec<(usize, WipAnchor)> = Vec::new();
    for &(target, ref head) in targets {
        let bottom = head.as_ref().and_then(|head| index.get(head)).copied();
        let anchor = graph::wip_anchor(
            rows.iter().map(|row| RowOccupancy {
                lane: row.lane,
                color: row.node_color,
            }),
            bottom,
        );
        anchors.push((target, anchor));
        if let (Some(bottom), Some(anchor)) = (bottom, anchor) {
            if let Some((deepest, _)) = drawn.iter_mut().find(|(_, existing)| *existing == anchor) {
                *deepest = (*deepest).max(bottom);
            } else {
                drawn.push((bottom, anchor));
            }
        }
    }
    for (bottom, anchor) in drawn {
        let color = wip_color(anchor.color);
        for row in &mut rows[..bottom] {
            row.edges.push(GraphEdge {
                from_lane: anchor.lane,
                to_lane: anchor.lane,
                kind: EdgeKind::Pass,
                color,
            });
        }
        rows[bottom].edges.push(GraphEdge {
            from_lane: anchor.lane,
            to_lane: anchor.lane,
            kind: EdgeKind::IntoNode,
            color,
        });
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

    /// Occupied HEAD columns remain valid; the painter places WIP behind history.
    #[test]
    fn keeps_heads_column_when_history_occupies_it() {
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
            vec![(WipTarget::Current, Some(WipAnchor { lane: 0, color: 4 }))],
            "history must not push WIP into a distant column"
        );

        // One Pass per row above HEAD, ending straight into its node.
        assert_eq!(ghosts(&rows[0]).len(), 1);
        assert_eq!(ghosts(&rows[1])[0].kind, EdgeKind::Pass);
        assert_eq!(ghosts(&rows[2])[0].kind, EdgeKind::Pass);
        let into = ghosts(&rows[3]);
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].kind, EdgeKind::IntoNode);
        assert_eq!(into[0].from_lane, 0);
        assert_eq!(into[0].to_lane, 0);
        assert_eq!(
            (0..3).filter(|&i| !ghosts(&rows[i]).is_empty()).count(),
            3,
            "Pass count must equal HEAD's row index"
        );
        assert!(rows.iter().all(|row| row.lane_count == 2));
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

    /// Reused columns retain each HEAD's colour without widening the graph.
    #[test]
    fn distinct_heads_can_share_a_column() {
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
                    Some(WipAnchor { lane: 1, color: 7 })
                ),
            ],
            "both anchors remain directly above their HEAD"
        );
        assert!(
            rows.iter().all(|r| r.lane_count == 2),
            "annotations must not widen the graph"
        );
    }

    #[test]
    fn identical_traces_extend_to_the_deepest_head_once() {
        let mut rows = vec![row(0, 0, 2), row(1, 1, 5), row(2, 0, 2), row(3, 1, 5)];
        let ix = index(&rows);
        inject_wip_edges(
            &mut rows,
            &[
                (WipTarget::Current, Some(id(1))),
                (WipTarget::Worktree(1), Some(id(3))),
            ],
            &ix,
        );
        for row in &rows[..3] {
            let trace = ghosts(row);
            assert_eq!(trace.len(), 1);
            assert_eq!(trace[0].kind, EdgeKind::Pass);
        }
        let landing = ghosts(&rows[3]);
        assert_eq!(landing.len(), 1);
        assert_eq!(landing[0].kind, EdgeKind::IntoNode);
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
