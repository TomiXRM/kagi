//! Commit graph lane drawing — T009 / T020 (rounded-corner edges + avatar node)
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
//!   - IntoNode: from (x_from, 0) descends vertically to corner, then arcs to
//!     (x_node, mid_y).  Corner is a quadratic Bézier (GitKraken style).
//!   - OutOfNode: from (x_node, mid_y) arcs to corner, then descends vertically
//!     to (x_to, ROW_H).
//!
//! # Corner-radius clamping (T020)
//!
//! `CORNER_R = 6.0` px.  Before drawing, R is clamped to:
//!   `R = min(CORNER_R, |dx| / 2, available_vertical / 2)`
//! so the curve never exceeds the available space regardless of lane spacing
//! or row height.

use gpui::{
    App, Bounds, Canvas, PathBuilder, Pixels, Window, canvas, hsla, point, px,
};

use kagi::graph::{EdgeKind, GraphEdge};

// ──────────────────────────────────────────────────────────────
// Layout constants
// ──────────────────────────────────────────────────────────────

/// Width of one lane column in pixels.
pub const LANE_W: f32 = 14.0;
/// Default maximum lanes to render when no explicit width is given.
/// T030: this is no longer the hard upper bound; `graph_canvas` now takes a
/// `visible_lanes` argument that replaces MAX_LANES for per-row clipping.
/// Retained for reference / GRAPH_COL_DEFAULT calculation.
#[allow(dead_code)]
pub const MAX_LANES: usize = 8;
/// Row height in pixels (must match what uniform_list computes for each row).
/// T008 rows use `py(px(3.))` (6 px total padding) plus text ≈ 18 px → 24 px.
pub const ROW_H: f32 = 29.0;  // 24.0 * 1.2 (user request: +20% row spacing)
/// Node circle radius in pixels.
const NODE_R: f32 = 4.0;
/// Edge stroke width in pixels.
const EDGE_W: f32 = 1.5;
/// Desired corner radius in pixels (T020). Will be clamped per-edge.
const CORNER_R: f32 = 6.0;

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

/// Compute the pixel width of the graph area for a given lane count,
/// using the default MAX_LANES cap (for legacy call sites).
/// T030: kept for reference; render_rows now uses `graph_col_w` directly.
#[allow(dead_code)]
pub fn graph_width(lane_count: usize) -> f32 {
    (lane_count.min(MAX_LANES) as f32) * LANE_W
}

/// Compute the pixel width for a given visible_lanes value (T030: column-resize aware).
#[allow(dead_code)]
pub fn graph_width_for_lanes(visible_lanes: usize) -> f32 {
    (visible_lanes as f32) * LANE_W
}

/// Compute how many lanes fit in a given pixel width (T030).
pub fn lanes_for_width(width_px: f32) -> usize {
    ((width_px / LANE_W).floor() as usize).max(0)
}

// ──────────────────────────────────────────────────────────────
// Rounded-corner edge helper (T020)
// ──────────────────────────────────────────────────────────────

/// Draw an **IntoNode** edge with a rounded corner.
///
/// Path:  `(x_from, y_top)` ↓ vertical → quadratic Bézier arc → `(x_node, mid_y)`
///
/// The corner bends at `mid_y` (row horizontal centre):
/// - vertical segment:  `(x_from, y_top)` → `(x_from, mid_y - R)`
/// - arc (quad Bézier): control at `(x_from, mid_y)`, end at `(x_node ± R, mid_y)`
///   where the sign depends on which side `x_node` is relative to `x_from`.
/// - horizontal segment: `(x_node ± R, mid_y)` → `(x_node, mid_y)`
///
/// The `R` value is clamped so neither half-segment underflows.
fn draw_into_node(
    builder: &mut PathBuilder,
    x_from: f32,
    y_top: f32,
    x_node: f32,
    mid_y: f32,
) {
    let dx = (x_node - x_from).abs();
    // Available vertical from y_top to mid_y.
    let avail_v = (mid_y - y_top).max(0.0);
    // Clamp R so curves fit within the available space.
    let r = CORNER_R.min(dx / 2.0).min(avail_v / 2.0);

    if r < 0.5 || dx < 0.5 {
        // Fallback: draw a straight diagonal line (from == node or very close).
        builder.move_to(point(px(x_from), px(y_top)));
        builder.line_to(point(px(x_node), px(mid_y)));
        return;
    }

    // Corner direction: is x_node to the right (+1) or left (-1) of x_from?
    let dir: f32 = if x_node > x_from { 1.0 } else { -1.0 };

    // Vertical segment: descend from y_top to the arc start.
    builder.move_to(point(px(x_from), px(y_top)));
    builder.line_to(point(px(x_from), px(mid_y - r)));

    // Quadratic Bézier: control at corner point, end at horizontal lane.
    // The arc begins at (x_from, mid_y - r) and ends at (x_from + dir*r, mid_y).
    let ctrl = point(px(x_from), px(mid_y));
    let end  = point(px(x_from + dir * r), px(mid_y));
    builder.curve_to(end, ctrl);

    // Horizontal segment from arc end to node centre.
    builder.line_to(point(px(x_node), px(mid_y)));
}

/// Draw an **OutOfNode** edge with a rounded corner.
///
/// Path:  `(x_node, mid_y)` → horizontal → quadratic Bézier arc → `(x_to, y_bot)` ↓
///
/// Mirror image of [`draw_into_node`].
fn draw_out_of_node(
    builder: &mut PathBuilder,
    x_node: f32,
    mid_y: f32,
    x_to: f32,
    y_bot: f32,
) {
    let dx = (x_to - x_node).abs();
    let avail_v = (y_bot - mid_y).max(0.0);
    let r = CORNER_R.min(dx / 2.0).min(avail_v / 2.0);

    if r < 0.5 || dx < 0.5 {
        // Fallback: straight diagonal.
        builder.move_to(point(px(x_node), px(mid_y)));
        builder.line_to(point(px(x_to), px(y_bot)));
        return;
    }

    // Corner direction.
    let dir: f32 = if x_to > x_node { 1.0 } else { -1.0 };

    // Start at node centre, run horizontal to arc start.
    builder.move_to(point(px(x_node), px(mid_y)));
    builder.line_to(point(px(x_to - dir * r), px(mid_y)));

    // Quadratic Bézier: control at corner point, end on vertical lane.
    let ctrl = point(px(x_to), px(mid_y));
    let end  = point(px(x_to), px(mid_y + r));
    builder.curve_to(end, ctrl);

    // Vertical segment from arc end to bottom of row.
    builder.line_to(point(px(x_to), px(y_bot)));
}

// ──────────────────────────────────────────────────────────────
// Per-row canvas element
// ──────────────────────────────────────────────────────────────

/// Return a `canvas` element that paints the graph lane for one commit row.
///
/// The returned [`Canvas<()>`] implements [`Styled`] so the caller can chain
/// `.size_full()`, `.w(...)`, etc. directly on the return value.
///
/// `visible_lanes` — how many lanes fit in the rendered column width.
/// Edges/nodes with lane indices >= visible_lanes are skipped so that no
/// drawing bleeds beyond the right edge of the graph column (T030).
///
/// `is_head` — whether this commit is the current HEAD (draws larger node + ring).
///
/// `is_merge` — whether this commit has 2+ parents (draws double-circle node).
///
/// `has_badges` — whether the badge column holds any badge chips for this row.
///   When true a thin horizontal connector line is drawn from lane 0's left
///   edge to the node centre (W2-GRAPH item 5: label→node connection).
pub fn graph_canvas(
    node_lane: usize,
    edges: Vec<GraphEdge>,
    visible_lanes: usize,
    is_head: bool,
    is_merge: bool,
    has_badges: bool,
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

            // Effective clip limit: skip lanes at or beyond this index.
            let clip = visible_lanes;

            // ── Draw edges ──────────────────────────────────
            for edge in &edges {
                // Skip edges entirely outside the clipped lane area (T030).
                if edge.from_lane >= clip && edge.to_lane >= clip {
                    continue;
                }

                let color = match edge.kind {
                    EdgeKind::IntoNode => lane_color(edge.from_lane),
                    EdgeKind::OutOfNode => lane_color(edge.to_lane),
                    EdgeKind::Pass => lane_color(edge.from_lane),
                };

                let mut builder = PathBuilder::stroke(px(EDGE_W));

                match edge.kind {
                    EdgeKind::Pass => {
                        // Straight vertical line, full row height.
                        let x = lane_x(edge.from_lane);
                        builder.move_to(point(px(x), px(oy)));
                        builder.line_to(point(px(x), px(oy + row_h)));
                    }
                    EdgeKind::IntoNode => {
                        let x_from = lane_x(edge.from_lane);
                        let x_node = lane_x(node_lane);
                        if edge.from_lane == node_lane {
                            // Same lane: straight vertical.
                            builder.move_to(point(px(x_from), px(oy)));
                            builder.line_to(point(px(x_node), px(mid_y)));
                        } else {
                            draw_into_node(&mut builder, x_from, oy, x_node, mid_y);
                        }
                    }
                    EdgeKind::OutOfNode => {
                        let x_node = lane_x(node_lane);
                        let x_to = lane_x(edge.to_lane);
                        if node_lane == edge.to_lane {
                            // Same lane: straight vertical.
                            builder.move_to(point(px(x_node), px(mid_y)));
                            builder.line_to(point(px(x_to), px(oy + row_h)));
                        } else {
                            draw_out_of_node(&mut builder, x_node, mid_y, x_to, oy + row_h);
                        }
                    }
                }

                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            }

            // ── Draw label→node connector line (W2-GRAPH item 5) ──
            // When the badge column has chips, draw a 1px horizontal line
            // from the left edge of the graph canvas (= right edge of badge
            // column) to the node centre.  The line uses the node's lane colour.
            // Only drawn when the node is in a visible lane.
            if has_badges && node_lane < clip {
                let x_node = lane_x(node_lane);
                // Draw from the left edge of the graph area (ox) to the node.
                // If the node is in lane 0 the line has zero length; only draw
                // when there is meaningful horizontal distance.
                if x_node > ox + 0.5 {
                    let color = lane_color(node_lane);
                    let mut builder = PathBuilder::stroke(px(1.0));
                    builder.move_to(point(px(ox), px(mid_y)));
                    builder.line_to(point(px(x_node), px(mid_y)));
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, color);
                    }
                }
            }

            // ── Draw node ● ─────────────────────────────────
            if node_lane < clip {
                let cx_abs = lane_x(node_lane);
                let color = lane_color(node_lane);

                // W2-GRAPH: HEAD node gets a larger radius + outer ring.
                // W2-GRAPH: merge node gets a double-circle (filled inner + stroked outer).
                let base_r = NODE_R;
                let head_r = base_r * 1.5; // 1.5× radius for HEAD

                const SEGMENTS: usize = 12;

                if is_head {
                    // HEAD: large filled circle + outer ring (same colour, slightly transparent).
                    // Outer ring (stroke).
                    let ring_r = head_r + 1.5;
                    let mut rb = PathBuilder::stroke(px(1.2));
                    for i in 0..=SEGMENTS {
                        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                        let px_val = cx_abs + ring_r * angle.cos();
                        let py_val = mid_y  + ring_r * angle.sin();
                        if i == 0 { rb.move_to(point(px(px_val), px(py_val))); }
                        else       { rb.line_to(point(px(px_val), px(py_val))); }
                    }
                    rb.close();
                    if let Ok(path) = rb.build() {
                        window.paint_path(path, color);
                    }
                    // Filled inner circle.
                    let mut fb = PathBuilder::fill();
                    for i in 0..=SEGMENTS {
                        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                        let px_val = cx_abs + head_r * angle.cos();
                        let py_val = mid_y  + head_r * angle.sin();
                        if i == 0 { fb.move_to(point(px(px_val), px(py_val))); }
                        else       { fb.line_to(point(px(px_val), px(py_val))); }
                    }
                    fb.close();
                    if let Ok(path) = fb.build() {
                        window.paint_path(path, color);
                    }
                } else if is_merge {
                    // Merge: double circle — stroked outer ring + stroked inner circle.
                    // Outer ring.
                    let outer_r = base_r + 2.5;
                    let mut rb = PathBuilder::stroke(px(1.2));
                    for i in 0..=SEGMENTS {
                        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                        let px_val = cx_abs + outer_r * angle.cos();
                        let py_val = mid_y  + outer_r * angle.sin();
                        if i == 0 { rb.move_to(point(px(px_val), px(py_val))); }
                        else       { rb.line_to(point(px(px_val), px(py_val))); }
                    }
                    rb.close();
                    if let Ok(path) = rb.build() {
                        window.paint_path(path, color);
                    }
                    // Filled inner circle (standard size).
                    let mut fb = PathBuilder::fill();
                    for i in 0..=SEGMENTS {
                        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                        let px_val = cx_abs + base_r * angle.cos();
                        let py_val = mid_y  + base_r * angle.sin();
                        if i == 0 { fb.move_to(point(px(px_val), px(py_val))); }
                        else       { fb.line_to(point(px(px_val), px(py_val))); }
                    }
                    fb.close();
                    if let Ok(path) = fb.build() {
                        window.paint_path(path, color);
                    }
                } else {
                    // Normal node: filled circle (existing behaviour).
                    let mut builder = PathBuilder::fill();
                    for i in 0..=SEGMENTS {
                        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                        let px_val = cx_abs + base_r * angle.cos();
                        let py_val = mid_y  + base_r * angle.sin();
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
            }
        },
    )
}
