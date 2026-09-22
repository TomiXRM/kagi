//! WIP anchors stay above HEAD, including when history occupies that column.
use super::*;

#[test]
fn intervening_nodes_do_not_push_the_anchor_away_from_head() {
    let rows = [
        RowOccupancy { lane: 2, color: 4 },
        RowOccupancy { lane: 1, color: 3 },
        RowOccupancy { lane: 2, color: 6 },
    ];
    let anchor = wip_anchor(rows.iter().copied(), Some(2)).unwrap();
    assert_eq!(anchor, WipAnchor { lane: 2, color: 6 });
}

#[test]
fn an_unresolved_or_unloaded_head_has_no_anchor() {
    let rows = [RowOccupancy { lane: 0, color: 0 }];
    assert_eq!(wip_anchor(rows.iter().copied(), None), None);
    assert_eq!(wip_anchor(rows.iter().copied(), Some(1)), None);
    assert_eq!(wip_anchor(std::iter::empty(), Some(0)), None);
}
