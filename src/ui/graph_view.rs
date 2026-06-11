//! Commit graph lane drawing — T009
//!
//! This module renders the visual commit graph (nodes ● and edges) into the
//! left-hand "graph area" of each commit row.  Drawing is done via the gpui
//! `canvas` element + `window.paint_path(PathBuilder::stroke / ::fill)`.
//!
//! # Coordinate system
//!
//! The canvas `paint` closure receives `bounds: Bounds<Pixels>` in *absolute*
//! window coordinates.  All points passed to `PathBuilder` must therefore be
//! in absolute coordinates: `bounds.origin.x + local_x`, etc.
//!
//! Row-local convention (y=0 at top, y=ROW_H at bottom):
//!   - Node ● sits at y = ROW_H / 2.
//!   - Edge top endpoint: y = 0 (= row top, connects to row above seamlessly).
//!   - Edge bottom endpoint: y = ROW_H (= row bottom, connects to row below).
//!   - Pass edges: vertical line from y=0 to y=ROW_H at the lane's x centre.
//!   - IntoNode: line from (from_lane x, 0) → (node x, ROW_H/2).
//!   - OutOfNode: line from (node x, ROW_H/2) → (to_lane x, ROW_H).

use gpui::{
    App, Bounds, Canvas, PathBuilder, Pixels, Window, canvas, hsla, point, px,
};

use crate::graph::{EdgeKind, GraphEdge};

// ──────────────────────────────────────────────────────────────
// Layout constants
// ──────────────────────────────────────────────────────────────

/// Width of one lane column in pixels.
pub const LANE_W: f32 = 14.0;
/// Maximum lanes to render (lanes beyond this are clipped).
pub const MAX_LANES: usize = 8;
/// Row height in pixels (must match what uniform_list computes for each row).
/// T008 rows use `py(px(3.))` (6 px total padding) plus text ≈ 18 px → 24 px.
pub const ROW_H: f32 = 24.0;
/// Node circle radius in pixels.
const NODE_R: f32 = 4.0;
/// Edge stroke width in pixels.
const EDGE_W: f32 = 1.5;

// ──────────────────────────────────────────────────────────────
// Lane colour palette (6 colours, Catppuccin-inspired)
// ──────────────────────────────────────────────────────────────

/// Return the HSLA colour for a given lane index (cycles through a palette).
fn lane_color(lane: usize) -> gpui::Hsla {
    // Hue values spaced evenly for 6 distinct colours (full saturation, mid
    // lightness, full opacity).
    const HUES: [f32; 6] = [
        0.583, // blue  (210°/360°)
        0.333, // green (120°/360°)
        0.083, // yellow/gold (30°/360°)
        0.917, // pink  (330°/360°)
        0.750, // purple (270°/360°)
        0.500, // cyan  (180°/360°)
    ];
    let h = HUES[lane % HUES.len()];
    hsla(h, 0.75, 0.65, 1.0)
}

// ──────────────────────────────────────────────────────────────
// Graph area width computation
// ──────────────────────────────────────────────────────────────

/// Compute the pixel width of the graph area for a given lane count.
pub fn graph_width(lane_count: usize) -> f32 {
    (lane_count.min(MAX_LANES) as f32) * LANE_W
}

// ──────────────────────────────────────────────────────────────
// Per-row canvas element
// ──────────────────────────────────────────────────────────────

/// Return a `canvas` element that paints the graph lane for one commit row.
///
/// The returned [`Canvas<()>`] implements [`Styled`] so the caller can chain
/// `.size_full()`, `.w(...)`, etc. directly on the return value.
pub fn graph_canvas(
    node_lane: usize,
    edges: Vec<GraphEdge>,
) -> Canvas<()> {
    canvas(
        // prepaint: nothing to measure
        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        // paint: draw edges first, node on top
        move |bounds: Bounds<Pixels>, _prepaint: (), window: &mut Window, _cx: &mut App| {
            let ox = f32::from(bounds.origin.x); // absolute left edge
            let oy = f32::from(bounds.origin.y); // absolute top edge
            // Use the actual canvas height rather than the ROW_H constant so
            // edges always span the full row even if the row height changes.
            let row_h = f32::from(bounds.size.height);
            let mid_y = oy + row_h / 2.0;

            // Helper: x-centre of a lane in absolute coords.
            let lane_x = |lane: usize| -> f32 {
                ox + (lane as f32) * LANE_W + LANE_W / 2.0
            };

            // ── Draw edges ──────────────────────────────────
            for edge in &edges {
                // Skip edges entirely outside the clipped lane area.
                if edge.from_lane >= MAX_LANES && edge.to_lane >= MAX_LANES {
                    continue;
                }

                let color = match edge.kind {
                    EdgeKind::IntoNode => lane_color(edge.from_lane),
                    EdgeKind::OutOfNode => lane_color(edge.to_lane),
                    EdgeKind::Pass => lane_color(edge.from_lane),
                };

                let (x0, y0, x1, y1) = match edge.kind {
                    EdgeKind::Pass => {
                        // Straight vertical line, full row height.
                        let x = lane_x(edge.from_lane);
                        (x, oy, x, oy + row_h)
                    }
                    EdgeKind::IntoNode => {
                        // From the top of from_lane → node centre.
                        (lane_x(edge.from_lane), oy, lane_x(node_lane), mid_y)
                    }
                    EdgeKind::OutOfNode => {
                        // From node centre → bottom of to_lane.
                        (lane_x(node_lane), mid_y, lane_x(edge.to_lane), oy + row_h)
                    }
                };

                let mut builder = PathBuilder::stroke(px(EDGE_W));
                builder.move_to(point(px(x0), px(y0)));
                builder.line_to(point(px(x1), px(y1)));
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            }

            // ── Draw node ● ─────────────────────────────────
            if node_lane < MAX_LANES {
                let cx_abs = lane_x(node_lane);
                let color = lane_color(node_lane);
                // Approximate circle with an 8-point polygon.
                const SEGMENTS: usize = 12;
                let mut builder = PathBuilder::fill();
                for i in 0..=SEGMENTS {
                    let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                    let px_val = cx_abs + NODE_R * angle.cos();
                    let py_val = mid_y + NODE_R * angle.sin();
                    if i == 0 {
                        builder.move_to(point(px(px_val), px(py_val)));
                    } else {
                        builder.line_to(point(px(px_val), px(py_val)));
                    }
                }
                builder.close();
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
}
