//! Graph rails in a bounded history window, using the commit list's existing layout.
//!
//! PR swimlanes and the restore preview share these columns, edge filtering and
//! node geometry. Their rows, labels, interactions and vertical windows remain
//! owned by the respective callers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use gpui::{div, prelude::*, px, Image, SharedString};
use kagi_domain::graph::GraphEdge;

use super::graph_view;
use super::theme::{self, theme};

pub(super) const CONTEXT_OPACITY: f32 = 0.45;
pub(super) const MAX_RAIL_LANES: usize = 6;

/// Reindex the lanes visible in one window without laying out the graph again.
pub(super) fn lane_columns(lanes: impl Iterator<Item = usize>) -> BTreeMap<usize, usize> {
    let used: BTreeSet<usize> = lanes.collect();
    used.into_iter().zip(0..).collect()
}

pub(super) fn column_of(columns: &BTreeMap<usize, usize>, lane: usize) -> usize {
    columns.get(&lane).copied().unwrap_or_else(|| {
        columns
            .range(..lane)
            .next_back()
            .map(|(_, column)| column + 1)
            .unwrap_or(0)
    })
}

pub(super) fn is_local_connector(edge: &GraphEdge) -> bool {
    edge.color == super::graph_squash::GHOST_COLOR
        || super::graph_wip::wip_color_index(edge.color).is_some()
}

pub(super) fn gutter_width(lanes: usize) -> f32 {
    graph_view::LANE_W * (lanes.clamp(1, MAX_RAIL_LANES) as f32 + 0.5)
}

pub(super) fn max_scroll(lanes: usize, rail: f32) -> f32 {
    (lanes as f32 * graph_view::LANE_W - rail).max(0.0)
}

pub(super) fn follow_scroll(lane: usize, rail: f32) -> f32 {
    let lane_w = graph_view::LANE_W;
    let left = lane as f32 * lane_w;
    let right = left + lane_w;
    if right > rail {
        right - rail
    } else {
        0.0
    }
}

pub(super) struct Rail<'a> {
    pub(super) width: f32,
    pub(super) scroll: f32,
    pub(super) columns: &'a BTreeMap<usize, usize>,
    pub(super) avatars: Option<&'a HashMap<String, Arc<Image>>>,
}

/// Paint an unchanged row of the repository's graph in a compact window.
/// Edge colours remain those assigned by the original layout, not the compact
/// column number; WIP/squash edges do not describe history in this window.
pub(super) fn render_rail(
    lane: usize,
    node_color: usize,
    edges: &[GraphEdge],
    node: graph_view::GraphNode,
    author: Option<(&str, &str)>,
    row_height: f32,
    rail: &Rail<'_>,
) -> gpui::Div {
    let column = column_of(rail.columns, lane);
    let lane_w = graph_view::lane_w();
    let node_cx = column as f32 * lane_w + lane_w / 2.0 - rail.scroll;
    let avatar = author.and_then(|(email, name)| {
        rail.avatars.map(|images| {
            let inner_d = theme::scaled(15.);
            let inner = div()
                .w(px(inner_d))
                .h(px(inner_d))
                .rounded_full()
                .overflow_hidden();
            let inner =
                match images.get(email).cloned() {
                    Some(image) => inner.child(
                        gpui::img(gpui::ImageSource::Image(image))
                            .size_full()
                            .rounded_full(),
                    ),
                    None => inner
                        .bg(kagi_ui_core::avatar::avatar_color(email))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_color(gpui::white()).text_xs().child(
                            SharedString::from(kagi_ui_core::avatar::avatar_initial(name)),
                        )),
                };
            let ring = graph_view::avatar_node_diameter();
            div()
                .absolute()
                .left(px(node_cx - ring / 2.))
                .top(px(theme::scaled(row_height) / 2. - ring / 2.))
                .w(px(ring))
                .h(px(ring))
                .rounded_full()
                .bg(theme().lane_color(node_color))
                .flex()
                .items_center()
                .justify_center()
                .child(inner)
        })
    });
    div()
        .w(theme::scaled_px(rail.width))
        .h_full()
        .flex_shrink_0()
        .relative()
        .overflow_hidden()
        .child(
            div().size_full().child(
                graph_view::graph_canvas(
                    column,
                    node_color,
                    edges
                        .iter()
                        .filter(|edge| !is_local_connector(edge))
                        .map(|edge| GraphEdge {
                            from_lane: column_of(rail.columns, edge.from_lane),
                            to_lane: column_of(rail.columns, edge.to_lane),
                            kind: edge.kind.clone(),
                            color: edge.color,
                        })
                        .collect(),
                    node,
                    false,
                    rail.scroll,
                    0.,
                    Vec::new(),
                )
                .size_full(),
            ),
        )
        .children(avatar)
}
