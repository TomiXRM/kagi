use super::*;
use crate::graph::EdgeKind;

struct Row {
    lane: usize,
    edges: Vec<GraphEdge>,
}

fn rows(lanes: &[usize]) -> Vec<Row> {
    lanes
        .iter()
        .map(|&lane| Row {
            lane,
            edges: vec![],
        })
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

fn choose(rows: &[Row], top: usize, bottom: usize, width: usize) -> Option<usize> {
    select_lane(rows, top, bottom, width, |row| SquashRow {
        lane: row.lane,
        edges: &row.edges,
    })
}

#[test]
fn free_tip_lane_wins_even_when_another_lane_is_closer_to_both_ends() {
    assert_eq!(choose(&rows(&[0, 0, 8]), 0, 2, 26), Some(8));
}

#[test]
fn unrelated_historical_width_does_not_determine_routing() {
    let rows = rows(&[0, 1, 0, 1]);
    assert_eq!(choose(&rows, 0, 3, 26), Some(2));
    assert_eq!(choose(&rows, 0, 3, 3), Some(2));
}

#[test]
fn blocked_tip_minimises_endpoint_detours_with_lower_lane_breaking_ties() {
    for (top, tip, expected) in [(0, 8, 4), (0, 5, 2), (8, 0, 4), (5, 0, 2)] {
        let rows = rows(&[top, tip, tip]);
        assert_eq!(choose(&rows, 0, 2, 26), Some(expected));
    }
    let mut rows = rows(&[0, 5, 5]);
    rows[1].edges.push(edge(2, 2, EdgeKind::Pass, 0));
    assert_eq!(choose(&rows, 0, 2, 26), Some(3));
}

#[test]
fn blocked_midpoints_keep_lower_lane_ties_and_respect_graph_boundaries() {
    for (squash, tip, blocked, width, expected) in [
        (0, 8, 4, 9, 3),
        (8, 0, 4, 9, 3),
        (6, 8, 7, 9, 6),
        (8, 6, 7, 9, 8),
        (0, 0, 0, 2, 1),
    ] {
        let mut rows = rows(&[squash, tip, tip]);
        rows[1]
            .edges
            .push(edge(blocked, blocked, EdgeKind::Pass, 0));
        assert_eq!(choose(&rows, 0, 2, width), Some(expected));
    }
}

#[test]
fn extreme_lane_indices_do_not_overflow_endpoint_midpoints() {
    for (squash, tip, expected) in [
        (usize::MAX - 4, usize::MAX - 2, usize::MAX - 3),
        (usize::MAX - 2, usize::MAX - 4, usize::MAX - 3),
        (usize::MAX - 3, usize::MAX - 2, usize::MAX - 3),
        (usize::MAX - 2, usize::MAX - 3, usize::MAX - 2),
    ] {
        let rows = rows(&[squash, tip, tip]);
        assert_eq!(choose(&rows, 0, 2, usize::MAX), Some(expected));
    }
}

#[test]
fn only_connecting_endpoint_halves_reserve_columns() {
    for color in [0, 7, usize::MAX, usize::MAX - 64] {
        for (at, kind, from, to, expected) in [
            (0, EdgeKind::IntoNode, 1, 0, 1),
            (0, EdgeKind::OutOfNode, 0, 1, 0),
            (0, EdgeKind::Pass, 1, 1, 0),
            (1, EdgeKind::OutOfNode, 1, 0, 1),
            (1, EdgeKind::IntoNode, 1, 1, 0),
            (1, EdgeKind::Pass, 1, 1, 0),
        ] {
            let mut rows = rows(&[0, 1]);
            rows[at].edges.push(edge(from, to, kind.clone(), color));
            assert_eq!(
                choose(&rows, 0, 1, 26),
                Some(expected),
                "endpoint {at}, kind {kind:?}, colour {color}"
            );
        }
    }
}

#[test]
fn middle_nodes_and_both_edge_halves_block_routing_regardless_of_colour() {
    for color in [0, 7, usize::MAX, usize::MAX - 64] {
        for (node, from, to, kind) in [
            (1, 0, 0, EdgeKind::Pass),
            (0, 1, 1, EdgeKind::Pass),
            (0, 1, 0, EdgeKind::IntoNode),
            (0, 0, 1, EdgeKind::OutOfNode),
        ] {
            let mut rows = rows(&[0, node, 1]);
            rows[1].edges.push(edge(from, to, kind, color));
            assert_eq!(choose(&rows, 0, 2, 26), Some(2));
        }
    }
}

#[test]
fn fully_occupied_interval_uses_one_new_high_water_lane() {
    let mut rows = rows(&[0, 0, 1]);
    for lane in 1..4 {
        rows[1].edges.push(edge(lane, lane, EdgeKind::Pass, 0));
    }
    assert_eq!(choose(&rows, 0, 2, 4), Some(4));
}

#[test]
fn adjacent_endpoints_can_reuse_a_lane_despite_unrelated_rows() {
    let mut rows = rows(&[0, 1, 1]);
    rows[0].edges.push(edge(0, 1, EdgeKind::OutOfNode, 0));
    rows[1].edges.push(edge(1, 1, EdgeKind::Pass, 0));
    // Row 0 is outside the second interval. Row 1's Pass occupies its lower
    // half, so lane 0 is the safe alternative despite the tip being on lane 1.
    assert_eq!(choose(&rows, 1, 2, 26), Some(0));
    assert_eq!(choose(&rows, 0, 1, 26), Some(0));
}

#[test]
fn invalid_or_unloaded_endpoints_have_no_route() {
    let rows = rows(&[0, 1]);
    for (top, bottom) in [(0, 0), (1, 0), (0, 2), (2, 3), (usize::MAX, 1)] {
        assert_eq!(choose(&rows, top, bottom, 26), None);
    }
    assert_eq!(choose(&[], 0, 1, 26), None);
}
