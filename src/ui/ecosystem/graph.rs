//! Coupling "Graph" rendering (ADR-0119): a force-directed graph where nodes
//! are files and edges are change-coupling. GPUI-native — edges are painted on
//! a full-size `canvas` (one path) and nodes are absolutely-positioned `div`s on
//! top, both reading the same normalized `[0,1]` positions from the pure
//! [`kagi_domain::coupling_graph`] layout, transformed by the view's zoom + pan.
//!
//! Interaction: scroll-wheel to zoom about the centre, left-drag to pan, and a
//! "Reset" button to refit.

use super::*;
use gpui::{
    canvas, point, relative, AnyElement, Bounds, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathBuilder, Pixels, ScrollDelta, ScrollWheelEvent,
};
use kagi_domain::coupling_graph::GraphNode;

/// Apply zoom-about-centre to a normalized coordinate (pan is added later, in px).
fn zoomed(v: f32, zoom: f32) -> f32 {
    0.5 + (v - 0.5) * zoom
}

/// Render the force-directed coupling graph (zoomable / pannable).
pub(super) fn render_coupling_graph(
    view: &EcosystemView,
    cx: &mut Context<EcosystemView>,
) -> AnyElement {
    let Some(graph) = view.data.coupling_graph.as_ref() else {
        return div().into_any_element();
    };
    let zoom = view.data.graph_zoom;
    let (pan_x, pan_y) = view.data.graph_pan;

    // Zoom-transformed fractions (pan is applied in px below + in the canvas).
    let positions: Vec<(f32, f32)> = graph
        .nodes
        .iter()
        .map(|n| (zoomed(n.x as f32, zoom), zoomed(n.y as f32, zoom)))
        .collect();
    let edges: Vec<(usize, usize)> = graph.edges.iter().map(|e| (e.a, e.b)).collect();
    let edge_color: Hsla = gpui::rgb(theme().text_muted).into();

    let canvas_positions = positions.clone();
    let bounds_cell = view.data.graph_bounds.clone();
    let edge_layer = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, _prepaint, window, _cx| {
            let ox = f32::from(bounds.origin.x);
            let oy = f32::from(bounds.origin.y);
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);
            // Record bounds so cursor-anchored zoom can map window px → graph.
            bounds_cell.set((ox, oy, w, h));
            let mut b = PathBuilder::stroke(theme::scaled_px(1.0));
            for &(a, c) in &edges {
                let (ax, ay) = canvas_positions[a];
                let (cx_, cy) = canvas_positions[c];
                b.move_to(point(px(ox + ax * w + pan_x), px(oy + ay * h + pan_y)));
                b.line_to(point(px(ox + cx_ * w + pan_x), px(oy + cy * h + pan_y)));
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, edge_color);
            }
        },
    )
    .absolute()
    .size_full();

    let max_deg = graph
        .nodes
        .iter()
        .map(|n| n.degree)
        .max()
        .unwrap_or(1)
        .max(1);

    // ── pan / zoom handlers ──────────────────────────────────────
    let wheel = cx.listener(|v, e: &ScrollWheelEvent, _w, cx| {
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(l) => l.y * 18.0,
        };
        let cursor = (f32::from(e.position.x), f32::from(e.position.y));
        v.graph_zoom_by(dy, cursor, cx);
    });
    let down = cx.listener(|v, e: &MouseDownEvent, _w, _cx| {
        v.graph_drag_start(f32::from(e.position.x), f32::from(e.position.y));
    });
    let moved = cx.listener(|v, e: &MouseMoveEvent, _w, cx| {
        if e.pressed_button == Some(MouseButton::Left) {
            v.graph_drag_move(f32::from(e.position.x), f32::from(e.position.y), cx);
        } else {
            v.graph_drag_end();
        }
    });
    let up = cx.listener(|v, _e: &MouseUpEvent, _w, _cx| v.graph_drag_end());
    let reset = cx.listener(|v, _e: &gpui::ClickEvent, _w, cx| v.graph_reset(cx));

    let mut container = div()
        .id("eco-graph")
        .relative()
        .size_full()
        .overflow_hidden()
        .bg(rgb(theme().bg_base))
        .on_scroll_wheel(wheel)
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_move(moved)
        .on_mouse_up(MouseButton::Left, up)
        .child(edge_layer);
    for (i, n) in graph.nodes.iter().enumerate() {
        let (fx, fy) = positions[i];
        container = container.child(node_dot(n, max_deg, fx, fy, pan_x, pan_y));
    }
    let reset_btn = div()
        .absolute()
        .top_2()
        .right_2()
        .id("eco-graph-reset")
        .px_2()
        .py_1()
        .rounded(theme::scaled_px(4.0))
        .bg(rgb(theme().surface))
        .text_size(theme::scaled_px(12.0))
        .text_color(rgb(theme().text_main))
        .cursor_pointer()
        .child(Msg::EcoResetView.t())
        .on_click(reset);
    container.child(reset_btn).into_any_element()
}

/// A node: a degree-sized dot at its (transformed) position, with its base name.
fn node_dot(n: &GraphNode, max_deg: u32, fx: f32, fy: f32, pan_x: f32, pan_y: f32) -> AnyElement {
    let frac = n.degree as f32 / max_deg as f32;
    let sz = 6.0 + frac * 10.0;
    let el = div()
        .absolute()
        .left(relative(fx))
        .top(relative(fy))
        .ml(px(pan_x - sz / 2.0))
        .mt(px(pan_y - sz / 2.0))
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
    let name = n.file.rsplit('/').next().unwrap_or(&n.file);
    el.child(
        div()
            .whitespace_nowrap()
            .text_size(theme::scaled_px(11.0))
            .text_color(rgb(theme().text_sub))
            .child(name.to_string()),
    )
    .into_any_element()
}
