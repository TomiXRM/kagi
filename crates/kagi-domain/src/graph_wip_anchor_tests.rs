//! Column/colour selection for the WIP → HEAD connectors ([`super::wip_anchor`]).
//!
//! Split out of `graph.rs` so that module stays under the repository's
//! per-file line ceiling (`ci/loc-baseline.txt`) — the same `#[path]` split
//! `list_filter` and `github` already use.

use super::*;

fn edge(from: usize, to: usize, kind: EdgeKind) -> GraphEdge {
    GraphEdge {
        from_lane: from,
        to_lane: to,
        kind,
        color: 0,
    }
}

fn row(lane: usize, color: usize, edges: &[GraphEdge]) -> RowOccupancy<'_> {
    RowOccupancy { lane, color, edges }
}

/// Nothing stands in HEAD's column, so the connector reuses it and the graph
/// does not get wider. The colour is HEAD's own lane colour, not the caller's
/// — that is what makes the line read as part of that branch.
#[test]
fn reuses_heads_column_when_it_is_empty_above() {
    let none: Vec<GraphEdge> = Vec::new();
    let rows = [row(0, 0, &none), row(0, 0, &none), row(1, 5, &none)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(2), 9),
        Some(WipAnchor { lane: 1, color: 5 })
    );
}

/// The connector enters from *outside* the graph — the WIP rows sit above row
/// 0 — so the very first row's **top** half has to be free as well. A line
/// that only arrives at row 0 (and is gone below it) still blocks the column.
#[test]
fn an_occupied_top_half_on_the_entry_row_forces_a_fresh_column() {
    let arrives = vec![edge(0, 1, EdgeKind::IntoNode)];
    let none: Vec<GraphEdge> = Vec::new();
    let rows = [row(1, 4, &arrives), row(0, 2, &none)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(1), 7),
        Some(WipAnchor { lane: 7, color: 2 })
    );
}

/// A line *leaving* a node downward into HEAD's column occupies only the
/// bottom half of its row, and would be overdrawn just the same.
#[test]
fn an_occupied_bottom_half_forces_a_fresh_column() {
    let leaves = vec![edge(1, 0, EdgeKind::OutOfNode)];
    let none: Vec<GraphEdge> = Vec::new();
    let rows = [row(1, 4, &leaves), row(1, 4, &none), row(0, 2, &none)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(2), 7),
        Some(WipAnchor { lane: 7, color: 2 })
    );
}

/// A commit node parked in HEAD's column above it blocks the column even when
/// no edge of that row mentions the lane (a root tip carries no edges at all).
#[test]
fn a_node_parked_in_heads_column_forces_a_fresh_one() {
    let none: Vec<GraphEdge> = Vec::new();
    let rows = [row(0, 2, &none), row(0, 2, &none)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(1), 3),
        Some(WipAnchor { lane: 3, color: 2 })
    );
}

/// HEAD's own top half counts: a branch line coming down HEAD's column into
/// the node is exactly the line the connector must not be drawn over, even
/// when HEAD is the first loaded row and there is nothing above it.
#[test]
fn heads_own_top_half_is_checked() {
    let into = vec![edge(0, 0, EdgeKind::IntoNode)];
    let rows = [row(0, 6, &into)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(0), 2),
        Some(WipAnchor { lane: 2, color: 6 })
    );
}

/// An unborn HEAD has no commit, and a HEAD outside the loaded window has no
/// row — both draw nothing rather than half a line, the rule stashes already
/// apply to an out-of-window base.
#[test]
fn an_unresolved_or_unloaded_head_has_no_anchor() {
    let none: Vec<GraphEdge> = Vec::new();
    let rows = [row(0, 0, &none), row(0, 0, &none)];
    assert_eq!(
        wip_anchor(rows.iter().copied(), None, 3),
        None,
        "unborn HEAD"
    );
    assert_eq!(
        wip_anchor(rows.iter().copied(), Some(2), 3),
        None,
        "past the window"
    );
    assert_eq!(
        wip_anchor(std::iter::empty(), Some(0), 3),
        None,
        "empty window"
    );
}
