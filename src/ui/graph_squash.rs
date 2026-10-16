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

use std::collections::{HashMap, HashSet};

use crate::graph::{self, EdgeKind, GraphEdge};
use gpui::{AppContext, Context};
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

/// Inject one dashed connector per proven squash merge. Returns how many were
/// drawn (links whose two ends are not both in the loaded window are skipped).
///
/// The tip's own lane is preferred when its connecting half-rows are free.
/// Otherwise reuse the nearest safe interval-local column before widening the
/// graph; unrelated history's maximum width must not dictate the detour.
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
    // Two branches can point at the same tip (`feat` and `feat-backup`), and
    // the links are per branch — without this the second one finds the tip's
    // lane busy with the first connector and takes a whole new column to draw
    // the identical line.
    let mut seen: HashSet<(&str, &str)> = HashSet::new();

    for link in links {
        if !seen.insert((link.tip.as_str(), link.squash.as_str())) {
            continue;
        }
        let (Some(&top), Some(&bottom)) = (
            index.get(&CommitId(link.squash.clone())),
            index.get(&CommitId(link.tip.clone())),
        ) else {
            continue;
        };
        // A stale index may be reversed or outside the loaded rows. The pure
        // selector skips those shapes before borrowing either endpoint.
        let Some(lane) = graph::squash::select_lane(rows, top, bottom, next_lane, |row| {
            graph::squash::SquashRow {
                lane: row.lane,
                edges: &row.edges,
            }
        }) else {
            continue;
        };
        if lane == next_lane {
            next_lane += 1;
        }
        let tip_lane = rows[bottom].lane;

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
        let Some(session) = self.active_session() else {
            return;
        };
        let read_key = self.reads.current_key(session);
        let Some(publish_gen) = self.ui.get(&session).map(|ui| ui.view_publish_gen) else {
            return;
        };

        let scan = async move {
            kagi_git::Backend::open(&repo_path).and_then(|b| b.collect_squash_links())
        };
        #[cfg(feature = "gui-e2e")]
        let task = super::e2e::take_squash_scan().unwrap_or_else(|| cx.background_spawn(scan));
        #[cfg(not(feature = "gui-e2e"))]
        let task = cx.background_spawn(scan);

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                // Superseded: another tab is on screen, a newer reload replaced
                // this session's graph, or a newer scan was launched.
                let still_ours = app.squash_gen == my_gen
                    && app.active_session() == Some(session)
                    && app.reads.is_fresh(read_key)
                    && app
                        .ui
                        .get(&session)
                        .is_some_and(|ui| ui.view_publish_gen == publish_gen);
                if !still_ours {
                    return;
                }
                let Ok(links) = result else {
                    return;
                };
                let index = app.view().commit_row_index.clone();
                let drawn = inject_squash_edges(&mut app.view_mut().rows, &links, &index);
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
    use crate::ui::graph_wip::{inject_wip_edges, wip_color, WipTarget};

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

    fn row_index(rows: &[CommitRow]) -> HashMap<CommitId, usize> {
        rows.iter()
            .enumerate()
            .map(|(index, row)| (row.id.clone(), index))
            .collect()
    }

    fn edge(from_lane: usize, to_lane: usize, kind: EdgeKind, color: usize) -> GraphEdge {
        GraphEdge {
            from_lane,
            to_lane,
            kind,
            color,
        }
    }

    fn assert_route(rows: &[CommitRow], top: usize, bottom: usize, lane: usize) {
        assert!(rows[top].edges.contains(&edge(
            rows[top].lane,
            lane,
            EdgeKind::OutOfNode,
            GHOST_COLOR,
        )));
        for row in &rows[top + 1..bottom] {
            assert!(row
                .edges
                .contains(&edge(lane, lane, EdgeKind::Pass, GHOST_COLOR)));
        }
        assert!(rows[bottom].edges.contains(&edge(
            lane,
            rows[bottom].lane,
            EdgeKind::IntoNode,
            GHOST_COLOR,
        )));
    }

    fn assert_history_unchanged(before: &[CommitRow], after: &[CommitRow]) {
        assert_eq!(before.len(), after.len());
        for (before, after) in before.iter().zip(after) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.lane, after.lane);
            assert_eq!(before.node_color, after.node_color);
            assert_eq!(before.parents, after.parents);
            assert_eq!(before.is_head, after.is_head);
            assert_eq!(before.is_merge, after.is_merge);
            assert_eq!(before.edges, after.edges[..before.edges.len()]);
            assert!(after.edges[before.edges.len()..]
                .iter()
                .all(|edge| edge.color == GHOST_COLOR));
        }
    }

    /// The whole point: a connector spans every row between the two ends, so
    /// the line is continuous instead of two floating stubs.
    #[test]
    fn connects_squash_commit_down_to_the_tip() {
        // Row 0 = squash commit on lane 0, row 3 = the dead-end tip on lane 1.
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 0), row(3, 1)];
        let index = row_index(&rows);

        assert_eq!(inject_squash_edges(&mut rows, &[link(3, 0)], &index), 1);

        assert_route(&rows, 0, 3, 1);
        assert!(rows.iter().all(|row| row.edges.len() == 1));
        assert!(rows.iter().all(|row| row.lane_count == 2));
    }

    #[test]
    fn blocked_tip_uses_local_free_lane_without_widening_history() {
        for width in [3, 26] {
            let mut rows = vec![row(0, 0), row(1, 1), row(2, 0), row(3, 1)];
            for row in &mut rows {
                row.lane_count = width;
            }
            let index = row_index(&rows);
            let before = rows.clone();
            assert_eq!(inject_squash_edges(&mut rows, &[link(3, 0)], &index), 1);
            assert_route(&rows, 0, 3, 2);
            assert_history_unchanged(&before, &rows);
            assert!(rows.iter().all(|row| row.lane_count == width));
        }
    }

    #[test]
    fn free_tip_precedes_nearer_columns_and_blocked_tip_uses_stable_ranking() {
        for (squash_lane, middle_lane, tip_lane, expected) in [
            (0, 0, 8, 8),
            (0, 8, 8, 4),
            (8, 0, 0, 4),
            (0, 5, 5, 2),
            (5, 0, 0, 2),
        ] {
            let mut rows = vec![row(0, squash_lane), row(1, middle_lane), row(2, tip_lane)];
            for row in &mut rows {
                row.lane_count = 26;
            }
            let index = row_index(&rows);
            assert_eq!(inject_squash_edges(&mut rows, &[link(2, 0)], &index), 1);
            assert_route(&rows, 0, 2, expected);
            assert!(rows.iter().all(|row| row.lane_count == 26));
        }
    }

    #[test]
    fn endpoints_reserve_only_the_half_rows_on_the_route() {
        for color in [0, wip_color(3), GHOST_COLOR] {
            for (at, kind, from, to, expected) in [
                (0, EdgeKind::IntoNode, 1, 0, 1),
                (0, EdgeKind::OutOfNode, 0, 1, 0),
                (0, EdgeKind::Pass, 1, 1, 0),
                (1, EdgeKind::OutOfNode, 1, 0, 1),
                (1, EdgeKind::IntoNode, 1, 1, 0),
                (1, EdgeKind::Pass, 1, 1, 0),
            ] {
                let mut rows = vec![row(0, 0), row(1, 1)];
                rows[at].edges.push(edge(from, to, kind, color));
                let index = row_index(&rows);
                let before = rows.clone();
                assert_eq!(inject_squash_edges(&mut rows, &[link(1, 0)], &index), 1);
                assert_route(&rows, 0, 1, expected);
                assert_history_unchanged(&before, &rows);
                assert!(rows.iter().all(|row| row.lane_count == 2));
            }
        }
    }

    #[test]
    fn intermediate_nodes_and_each_edge_kind_block_even_annotation_colours() {
        for color in [0, 7, wip_color(3), GHOST_COLOR] {
            for (node_lane, from, to, kind) in [
                (1, 0, 0, EdgeKind::Pass),
                (0, 1, 1, EdgeKind::Pass),
                (0, 1, 0, EdgeKind::IntoNode),
                (0, 0, 1, EdgeKind::OutOfNode),
            ] {
                let mut rows = vec![row(0, 0), row(1, node_lane), row(2, 1)];
                for row in &mut rows {
                    row.lane_count = 4;
                }
                rows[1].edges.push(edge(from, to, kind, color));
                let index = row_index(&rows);
                let before = rows.clone();
                assert_eq!(inject_squash_edges(&mut rows, &[link(2, 0)], &index), 1);
                assert_route(&rows, 0, 2, 2);
                assert_history_unchanged(&before, &rows);
                assert!(rows.iter().all(|row| row.lane_count == 4));
            }
        }
    }

    #[test]
    fn fully_occupied_interval_widens_once_without_moving_history_or_wip() {
        let mut rows = vec![row(0, 0), row(1, 0), row(2, 1)];
        for row in &mut rows {
            row.lane_count = 3;
            row.node_color = row.lane + 4;
        }
        let parent = rows[1].id.clone();
        rows[0].parents.push(parent);
        rows[0].edges.push(edge(0, 0, EdgeKind::OutOfNode, 4));
        rows[1].edges.push(edge(0, 0, EdgeKind::IntoNode, 4));
        rows[1].edges.push(edge(0, 0, EdgeKind::OutOfNode, 4));
        rows[1].edges.push(edge(2, 2, EdgeKind::Pass, 7));
        rows[2].is_head = true;
        let index = row_index(&rows);
        let targets = [(WipTarget::Current, Some(rows[2].id.clone()))];
        let anchors = inject_wip_edges(&mut rows, &targets, &index);
        let before = rows.clone();

        assert_eq!(inject_squash_edges(&mut rows, &[link(2, 0)], &index), 1);

        assert_route(&rows, 0, 2, 3);
        assert_history_unchanged(&before, &rows);
        assert!(rows.iter().all(|row| row.lane_count == 4));
        let anchor = graph::wip_anchor(
            rows.iter().map(|row| graph::RowOccupancy {
                lane: row.lane,
                color: row.node_color,
            }),
            Some(2),
        );
        assert_eq!(anchors, vec![(WipTarget::Current, anchor)]);
    }

    #[test]
    fn overlapping_routes_reserve_previously_injected_endpoint_and_pass_edges() {
        for (second_top, second_tip) in [(0, 5), (2, 5), (1, 5)] {
            let mut rows = vec![
                row(0, 0),
                row(1, 1),
                row(2, 0),
                row(3, 1),
                row(4, 0),
                row(5, 1),
            ];
            for row in &mut rows {
                row.lane_count = 26;
            }
            let index = row_index(&rows);
            let links = [link(3, 0), link(second_tip, second_top)];
            assert_eq!(inject_squash_edges(&mut rows, &links, &index), 2);
            assert_route(&rows, 0, 3, 2);
            assert_route(&rows, second_top, second_tip, 3);
            assert!(rows.iter().all(|row| row.lane_count == 26));
        }
    }

    #[test]
    fn disjoint_routes_reuse_existing_or_new_high_water_lane() {
        for width in [2, 26] {
            let mut rows = vec![
                row(0, 0),
                row(1, 1),
                row(2, 0),
                row(3, 1),
                row(4, 0),
                row(5, 1),
                row(6, 0),
                row(7, 1),
            ];
            for row in &mut rows {
                row.lane_count = width;
            }
            let index = row_index(&rows);
            let before = rows.clone();
            assert_eq!(
                inject_squash_edges(&mut rows, &[link(3, 0), link(7, 4)], &index),
                2
            );
            assert_route(&rows, 0, 3, 2);
            assert_route(&rows, 4, 7, 2);
            assert_history_unchanged(&before, &rows);
            assert!(rows.iter().all(|row| row.lane_count == width.max(3)));
        }
    }

    #[test]
    fn routes_meeting_at_one_node_reuse_opposite_endpoint_halves() {
        let mut rows = vec![
            row(0, 0),
            row(1, 1),
            row(2, 0),
            row(3, 1),
            row(4, 0),
            row(5, 1),
            row(6, 0),
        ];
        let index = row_index(&rows);
        assert_eq!(
            inject_squash_edges(&mut rows, &[link(3, 0), link(6, 3)], &index),
            2
        );
        assert_route(&rows, 0, 3, 2);
        assert_route(&rows, 3, 6, 2);
        assert!(rows.iter().all(|row| row.lane_count == 3));
    }

    #[test]
    fn real_stable_layout_keeps_node_colours_parents_and_wip_edges() {
        use kagi_domain::commit::{Commit, Signature};

        let signature = Signature {
            name: "Graph test".into(),
            email: "graph@example.test".into(),
            time: 0,
        };
        let commits: Vec<Commit> = [(0, Some(1)), (1, Some(3)), (2, Some(3)), (3, None)]
            .into_iter()
            .map(|(id, parent)| Commit {
                id: CommitId(format!("{id:040}")),
                parents: parent
                    .into_iter()
                    .map(|id| CommitId(format!("{id:040}")))
                    .collect(),
                author: signature.clone(),
                committer: signature.clone(),
                summary: "Stable history".into(),
                message: "Stable history".into(),
            })
            .collect();
        let layout = graph::layout(&commits);
        let width = layout.lane_count;
        let mut rows: Vec<CommitRow> = commits
            .iter()
            .zip(layout.rows)
            .map(|(commit, layout)| {
                let mut row = CommitRow::empty_for_test(commit.id.clone());
                row.lane = layout.lane;
                row.node_color = layout.color;
                row.edges = layout.edges;
                row.lane_count = width;
                row.parents = commit.parents.clone();
                row
            })
            .collect();
        let index = row_index(&rows);
        let targets = [(WipTarget::Current, Some(rows[3].id.clone()))];
        let anchors = inject_wip_edges(&mut rows, &targets, &index);
        assert_eq!(anchors.len(), 1);
        assert!(anchors[0].1.is_some());
        let before = rows.clone();

        assert_eq!(inject_squash_edges(&mut rows, &[link(2, 0)], &index), 1);

        assert_route(&rows, 0, 2, 1);
        assert_history_unchanged(&before, &rows);
        assert!(rows.iter().all(|row| row.lane_count == width));
    }

    #[test]
    fn branch_aliases_draw_one_complete_connector() {
        let mut rows = vec![row(0, 0), row(1, 1), row(2, 0), row(3, 1)];
        for row in &mut rows {
            row.lane_count = 26;
        }
        let index = row_index(&rows);
        let first = link(3, 0);
        let mut alias = first.clone();
        alias.branch = "feat-backup".into();
        assert_eq!(inject_squash_edges(&mut rows, &[first, alias], &index), 1);
        assert_route(&rows, 0, 3, 2);
        assert!(rows.iter().all(|row| row.edges.len() == 1));
        assert!(rows.iter().all(|row| row.lane_count == 26));
    }

    #[test]
    fn empty_missing_reversed_and_out_of_bounds_indices_leave_history_untouched() {
        assert_eq!(
            inject_squash_edges(&mut [], &[link(1, 0)], &HashMap::new()),
            0
        );
        for endpoints in [
            None,
            Some((1, 0)),
            Some((0, 0)),
            Some((0, 2)),
            Some((2, 3)),
            Some((usize::MAX, 1)),
            Some((0, usize::MAX)),
        ] {
            let mut rows = vec![row(0, 0), row(1, 1)];
            rows[0].edges.push(edge(0, 0, EdgeKind::OutOfNode, 4));
            let mut index = HashMap::new();
            if let Some((top, bottom)) = endpoints {
                index.insert(rows[0].id.clone(), top);
                index.insert(rows[1].id.clone(), bottom);
            }
            let before = rows.clone();
            assert_eq!(inject_squash_edges(&mut rows, &[link(1, 0)], &index), 0);
            assert_history_unchanged(&before, &rows);
            for (before, after) in before.iter().zip(&rows) {
                assert_eq!(before.edges, after.edges);
                assert_eq!(before.lane_count, after.lane_count);
            }
        }
    }

    /// A link pointing at a commit outside the loaded window draws nothing
    /// rather than half a line.
    #[test]
    fn skips_a_link_that_is_not_fully_loaded() {
        let mut rows = vec![row(0, 0), row(1, 1)];
        let index = row_index(&rows);
        assert_eq!(
            inject_squash_edges(&mut rows, &[link(1, 99), link(99, 0)], &index),
            0
        );
        assert!(rows.iter().all(|r| r.edges.is_empty()));
    }
}
