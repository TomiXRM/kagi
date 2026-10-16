//! Interval-local routing for squash connectors, independent of commit layout.

use super::{lane_bottom_busy, lane_top_busy, GraphEdge};

/// Borrow only the occupancy needed for squash routing, including previously
/// injected connectors. The owner retains its nodes, colours and edge storage.
#[derive(Clone, Copy)]
pub struct SquashRow<'a> {
    pub lane: usize,
    pub edges: &'a [GraphEdge],
}

/// Prefer the tip's lane; otherwise minimise the longest endpoint detour, then
/// total endpoint distance, then lane number. Allocate `lane_count` only when
/// every existing column is occupied over this interval.
///
/// `lane_count` is an exclusive graph-wide upper bound on every node/edge lane,
/// including connectors already injected. The caller raises it only when this
/// function returns that new column. Invalid endpoint indices draw nothing.
///
/// Only the squash row's lower half and tip row's upper half are on the route;
/// intermediate nodes and both edge halves reserve their columns, regardless
/// of colour (real history, WIP, stash and previously injected ghosts alike).
///
/// The projection borrows the owner's rows; routing allocates and copies
/// nothing. Candidates are visited in rank order, stopping at the first safe
/// lane: O(examined lanes × interval occupancy), O(1) extra space.
pub fn select_lane<'a, R>(
    rows: &'a [R],
    top: usize,
    bottom: usize,
    lane_count: usize,
    project: impl Fn(&'a R) -> SquashRow<'a>,
) -> Option<usize> {
    if top >= bottom || bottom >= rows.len() {
        return None;
    }
    let squash = project(&rows[top]);
    let tip = project(&rows[bottom]);
    let free = |lane| {
        !lane_bottom_busy(squash.edges, lane)
            && !lane_top_busy(tip.edges, lane)
            && rows[top + 1..bottom].iter().all(|row| {
                let row = project(row);
                row.lane != lane
                    && !lane_top_busy(row.edges, lane)
                    && !lane_bottom_busy(row.edges, lane)
            })
    };
    if free(tip.lane) {
        return Some(tip.lane);
    }

    // Maximum endpoint distance increases outwards from their midpoint.
    // Symmetric columns also tie on total distance, so visit the lower one
    // first. Computing from the span avoids overflowing an endpoint sum.
    let span = squash.lane.abs_diff(tip.lane);
    let lower_middle = squash.lane.min(tip.lane) + span / 2;
    let upper_middle = lower_middle + span % 2;
    let mut left = lane_count.checked_sub(1).map(|last| lower_middle.min(last));
    let mut right = Some(upper_middle).filter(|&lane| lane < lane_count);
    while left.is_some() || right.is_some() {
        if let Some(lane) = left {
            if lane != tip.lane && free(lane) {
                return Some(lane);
            }
        }
        if let Some(lane) = right {
            // An even midpoint belongs to both traversals only on their first
            // iteration; it must not trigger a second occupancy scan.
            if right != left && lane != tip.lane && free(lane) {
                return Some(lane);
            }
        }
        left = left.and_then(|lane| lane.checked_sub(1));
        right = right
            .and_then(|lane| lane.checked_add(1))
            .filter(|&lane| lane < lane_count);
    }
    Some(lane_count)
}

#[cfg(test)]
#[path = "graph_squash_tests.rs"]
mod tests;
