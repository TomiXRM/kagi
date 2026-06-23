//! Coupling "Graph" rendering (ADR-0119): a force-directed graph where nodes
//! are files and edges are change-coupling. GPUI-native — edges are painted on
//! a full-size `canvas` (one path) and nodes are absolutely-positioned `div`s on
//! top, both reading the same normalized `[0,1]` positions from the pure
//! [`kagi_domain::coupling_graph`] layout.

use super::*;
use gpui::{canvas, point, relative, AnyElement, Bounds, Hsla, PathBuilder, Pixels};
use kagi_domain::coupling_graph::{CouplingGraph, GraphNode};

/// How many of the highest-degree nodes get a text label (avoids a hairball).
const LABELS: usize = 14;

/// Render the force-directed coupling graph.
pub(super) fn render_coupling_graph(graph: &CouplingGraph) -> AnyElement {
    // Positions + edges captured by value for the canvas paint closure.
    let positions: Vec<(f32, f32)> = graph
        .nodes
        .iter()
        .map(|n| (n.x as f32, n.y as f32))
        .collect();
    let edges: Vec<(usize, usize)> = graph.edges.iter().map(|e| (e.a, e.b)).collect();
    let edge_color: Hsla = gpui::rgb(theme().text_muted).into();

    let edge_layer = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, _prepaint, window, _cx| {
            let ox = f32::from(bounds.origin.x);
            let oy = f32::from(bounds.origin.y);
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);
            let mut b = PathBuilder::stroke(theme::scaled_px(1.0));
            for &(a, c) in &edges {
                let (ax, ay) = positions[a];
                let (cx_, cy) = positions[c];
                b.move_to(point(px(ox + ax * w), px(oy + ay * h)));
                b.line_to(point(px(ox + cx_ * w), px(oy + cy * h)));
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, edge_color);
            }
        },
    )
    .absolute()
    .size_full();

    // Label the highest-degree nodes only.
    let max_deg = graph
        .nodes
        .iter()
        .map(|n| n.degree)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut order: Vec<usize> = (0..graph.nodes.len()).collect();
    order.sort_by(|&i, &j| graph.nodes[j].degree.cmp(&graph.nodes[i].degree));
    let labelled: std::collections::HashSet<usize> = order.into_iter().take(LABELS).collect();

    let mut container = div()
        .id("eco-graph")
        .relative()
        .size_full()
        .overflow_hidden()
        .bg(rgb(theme().bg_base))
        .child(edge_layer);
    for (i, n) in graph.nodes.iter().enumerate() {
        container = container.child(node_dot(n, max_deg, labelled.contains(&i)));
    }
    container.into_any_element()
}

/// A node: a degree-sized dot at its normalized position, optionally labelled.
fn node_dot(n: &GraphNode, max_deg: u32, labelled: bool) -> AnyElement {
    let frac = n.degree as f32 / max_deg as f32;
    let sz = 6.0 + frac * 10.0;
    let mut el = div()
        .absolute()
        .left(relative(n.x as f32))
        .top(relative(n.y as f32))
        .ml(px(-(sz / 2.0)))
        .mt(px(-(sz / 2.0)))
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .flex_shrink_0()
                .w(px(sz))
                .h(px(sz))
                .rounded_full()
                .bg(rgb(theme().accent)),
        );
    if labelled {
        let name = n.file.rsplit('/').next().unwrap_or(&n.file);
        el = el.child(
            div()
                .whitespace_nowrap()
                .text_size(theme::scaled_px(11.0))
                .text_color(rgb(theme().text_sub))
                .child(name.to_string()),
        );
    }
    el.into_any_element()
}
