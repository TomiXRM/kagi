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
//! `CORNER_R = 9.0` px.  Before drawing, R is clamped to:
//!   `R = min(CORNER_R, |dx| / 2, available_vertical / 2)`
//! so the curve never exceeds the available space regardless of lane spacing
//! or row height.

use gpui::{canvas, point, px, App, Bounds, Canvas, PathBuilder, Pixels, Window};

use crate::graph::{EdgeKind, GraphEdge};

use crate::ui::graph_squash::GHOST_COLOR;
use crate::ui::theme::{self, theme};

// ──────────────────────────────────────────────────────────────
// Layout constants
// ──────────────────────────────────────────────────────────────

/// Base (1.0× zoom) width of one lane column in pixels.
///
/// W28: this is the *unscaled* source of truth.  All live geometry and the
/// column-width <-> lane-count conversions go through [`lane_w`] so lane spacing
/// tracks `theme::zoom()` uniformly with the row text/height.
pub const LANE_W: f32 = 22.0;
/// Row height in pixels (must match what uniform_list computes for each row).
/// T008 rows use `py(px(3.))` (6 px total padding) plus text ≈ 18 px → 24 px.
pub const ROW_H: f32 = 29.0; // 24.0 * 1.2 (user request: +20% row spacing)
/// Node circle radius in pixels.
const NODE_R: f32 = 4.0;
/// Edge stroke width in pixels.
const EDGE_W: f32 = 1.5;
/// Desired corner radius in pixels (T020). Will be clamped per-edge.
const CORNER_R: f32 = 9.0;

/// W28: zoom-scaled lane width — `LANE_W * zoom()`.
///
/// Every lane-spacing computation (lane x-centres, column<->lane conversions,
/// horizontal scroll steps) goes through here so the graph's horizontal pitch
/// scales by the same factor as the row height and text.
#[inline]
pub fn lane_w() -> f32 {
    theme::scaled(LANE_W)
}

// ──────────────────────────────────────────────────────────────
// Lane colour palette (6 colours, Catppuccin-inspired)
// ──────────────────────────────────────────────────────────────

/// Return the HSLA colour for a given lane index (cycles through the active
/// theme's 6-colour lane palette — W9-THEME / ADR-0036).
fn lane_color(lane: usize) -> gpui::Hsla {
    theme().lane_color(lane)
}

// ──────────────────────────────────────────────────────────────
// Graph area width computation
// ──────────────────────────────────────────────────────────────

/// Compute how many lanes fit in a given pixel width (T030).
///
/// W28: uses the zoom-scaled [`lane_w`] so the visible-lane count stays
/// consistent with the actual on-screen lane pitch at any zoom — otherwise the
/// canvas would clip a different number of lanes than it draws.
pub fn lanes_for_width(width_px: f32) -> usize {
    (width_px / lane_w()).floor() as usize
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
fn draw_into_node(builder: &mut PathBuilder, x_from: f32, y_top: f32, x_node: f32, mid_y: f32) {
    let dx = (x_node - x_from).abs();
    // Available vertical from y_top to mid_y.
    let avail_v = (mid_y - y_top).max(0.0);
    // Clamp R so curves fit within the available space: the vertical segment
    // needs r <= avail_v and the horizontal one r <= dx (at the limit the
    // straight run just vanishes and the arc spans the whole leg). The old
    // /2 clamps silently capped the radius at ~7px regardless of CORNER_R.
    // W28: corner radius scales with zoom so the bend keeps its proportion to
    // the (scaled) lane spacing and row height.
    let r = theme::scaled(CORNER_R).min(dx).min(avail_v);

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
    let end = point(px(x_from + dir * r), px(mid_y));
    builder.curve_to(end, ctrl);

    // Horizontal segment from arc end to node centre.
    builder.line_to(point(px(x_node), px(mid_y)));
}

/// Draw an **OutOfNode** edge with a rounded corner.
///
/// Path:  `(x_node, mid_y)` → horizontal → quadratic Bézier arc → `(x_to, y_bot)` ↓
///
/// Mirror image of [`draw_into_node`].
fn draw_out_of_node(builder: &mut PathBuilder, x_node: f32, mid_y: f32, x_to: f32, y_bot: f32) {
    let dx = (x_to - x_node).abs();
    let avail_v = (y_bot - mid_y).max(0.0);
    // W28: corner radius scales with zoom; clamp bounds as in `draw_into_node`.
    let r = theme::scaled(CORNER_R).min(dx).min(avail_v);

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
    let end = point(px(x_to), px(mid_y + r));
    builder.curve_to(end, ctrl);

    // Vertical segment from arc end to bottom of row.
    builder.line_to(point(px(x_to), px(y_bot)));
}

/// Draw a **shift** edge: a lane that moved from `x_from` (top) to `x_to`
/// (bottom) because compaction reclaimed a column between rows.
///
/// Path: vertical down at `x_from` → rounded corner → horizontal at mid-row →
/// rounded corner → vertical down at `x_to`.  An S-step, mirroring the corner
/// style of [`draw_into_node`] / [`draw_out_of_node`].
fn draw_shift(builder: &mut PathBuilder, x_from: f32, y_top: f32, x_to: f32, y_bot: f32) {
    let dx = (x_to - x_from).abs();
    let mid_y = (y_top + y_bot) / 2.0;
    let avail_v = (mid_y - y_top).min(y_bot - mid_y).max(0.0);
    let r = theme::scaled(CORNER_R).min(dx / 2.0).min(avail_v);

    if r < 0.5 || dx < 0.5 {
        // Degenerate: draw a straight diagonal.
        builder.move_to(point(px(x_from), px(y_top)));
        builder.line_to(point(px(x_to), px(y_bot)));
        return;
    }

    let dir: f32 = if x_to > x_from { 1.0 } else { -1.0 };

    builder.move_to(point(px(x_from), px(y_top)));
    builder.line_to(point(px(x_from), px(mid_y - r)));
    // Top corner: arc from vertical into the horizontal run.
    builder.curve_to(
        point(px(x_from + dir * r), px(mid_y)),
        point(px(x_from), px(mid_y)),
    );
    // Horizontal run across to the destination column.
    builder.line_to(point(px(x_to - dir * r), px(mid_y)));
    // Bottom corner: arc from horizontal back into the vertical.
    builder.curve_to(point(px(x_to), px(mid_y + r)), point(px(x_to), px(mid_y)));
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
#[allow(clippy::too_many_arguments)]
/// Paint a vertical dashed segment. Used for ghost connectors (ADR-0139),
/// which stand for patch-id equivalence rather than a parent link — a solid
/// line there would claim a relationship git does not have.
///
/// Each dash is its own path: `PathBuilder::stroke` is used for one stroke at
/// a time everywhere else in this file, and four short paints per row is
/// cheaper than reasoning about multi-subpath strokes.
fn paint_dashed_vertical(
    window: &mut Window,
    x: f32,
    y_top: f32,
    y_bottom: f32,
    color: gpui::Hsla,
) {
    let dash = theme::scaled(4.0);
    let gap = theme::scaled(3.0);
    let mut y = y_top;
    while y < y_bottom {
        let y2 = (y + dash).min(y_bottom);
        let mut b = PathBuilder::stroke(theme::scaled_px(EDGE_W));
        b.move_to(point(px(x), px(y)));
        b.line_to(point(px(x), px(y2)));
        if let Ok(path) = b.build() {
            window.paint_path(path, color);
        }
        y = y2 + gap;
    }
}

pub fn graph_canvas(
    node_lane: usize,
    // Stable colour index for this node's lane (carried with the branch). Used
    // for the ● node and the label→node connector; edges carry their own colour.
    node_color: usize,
    edges: Vec<GraphEdge>,
    is_head: bool,
    is_merge: bool,
    has_badges: bool,
    // kagi: horizontal scroll offset in px — lanes hidden by a narrow column
    // can be brought into view by scrolling the graph column sideways.
    scroll_x: f32,
    // kagi: left pad (px) applied to the lane geometry so lane 0 clears the
    // column's left edge (room for the avatar node). The label→node connector
    // still starts at the true left edge, so it doesn't gap with the
    // badge-column connector.
    pad_l: f32,
    // ADR-0088: lanes that belong to stash branch lines. Nodes/edges on these
    // lanes are painted in the stash colour (yellow) to stand out.
    stash_lanes: Vec<usize>,
) -> Canvas<()> {
    let stash_color: gpui::Hsla = gpui::rgb(theme().color_warning).into();
    // ADR-0139: ghost connectors are drawn back, not loud — the real history is
    // the thing being read; this only explains a gap in it.
    let ghost_color: gpui::Hsla = {
        let mut c: gpui::Hsla = gpui::rgb(theme().text_muted).into();
        c.a = 0.55;
        c
    };
    let is_stash_lane = move |lane: usize| stash_lanes.contains(&lane);
    canvas(
        // prepaint: nothing to measure
        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        // paint: draw edges first, node on top. The whole body runs under a
        // content mask = the canvas bounds: the canvas would otherwise paint
        // outside its bounds in both directions. Masking (instead of the old
        // skip-whole-edge lane culling) means an edge whose other end is
        // horizontally scrolled out of view still draws its visible part —
        // scrolling right used to erase every line touching an off-screen
        // lane, including the row's own node curves (user report). Vertical
        // culling is unaffected (uniform_list only renders visible rows).
        move |bounds: Bounds<Pixels>, _prepaint: (), window: &mut Window, _cx: &mut App| {
            let mask = gpui::ContentMask { bounds };
            window.with_content_mask(Some(mask), |window| {
                let ox = f32::from(bounds.origin.x); // absolute left edge
                let oy = f32::from(bounds.origin.y); // absolute top edge
                                                     // Use the actual canvas height rather than the ROW_H constant so
                                                     // edges always span the full row even if the row height changes.
                                                     // W28: use the measured row height for vertical anchoring. The row
                                                     // container height is itself `scaled_px(row_height)`, so `mid_y`
                                                     // (and thus the ● node centre + edge endpoints) tracks zoom with no
                                                     // extra scaling here — that is what keeps the node centred and the
                                                     // edges drift-free at any zoom.
                let row_h = f32::from(bounds.size.height);
                let mid_y = node_center_y(oy, row_h);

                // Helper: x-centre of a lane in absolute coords (scroll-aware).
                // Shares `lane_center_x` with the unit tests so the drawn geometry
                // and the asserted geometry are guaranteed identical. `pad_l` shifts
                // the lanes right; the connector below still anchors at `ox`.
                let lane_x = |lane: usize| -> f32 { lane_center_x(ox + pad_l, lane, scroll_x) };

                // ── Draw edges (mask does the clipping) ─────────
                for edge in &edges {
                    // A ghost connector is marked on the edge, not the lane: it
                    // deliberately reuses a real column (the tip's dead one), so
                    // the stash-style per-lane test cannot express it.
                    let ghost = edge.color == GHOST_COLOR;
                    // Colour comes from the edge's carried (branch-stable) colour
                    // index — not the column index — so a branch keeps its colour
                    // even when compaction shifts its lane.
                    let color = if ghost {
                        ghost_color
                    } else if is_stash_lane(edge.from_lane) || is_stash_lane(edge.to_lane) {
                        stash_color
                    } else {
                        lane_color(edge.color)
                    };

                    // Every straight run of a connector is dashed, so it never
                    // reads as a parent link. Its two end arcs stay solid — a
                    // dashed curve is not worth the arc-length maths — but keep
                    // the muted colour.
                    if ghost {
                        let seg = match edge.kind {
                            EdgeKind::Pass if edge.from_lane == edge.to_lane => {
                                Some((lane_x(edge.from_lane), oy, oy + row_h))
                            }
                            EdgeKind::IntoNode if edge.from_lane == node_lane => {
                                Some((lane_x(edge.from_lane), oy, mid_y))
                            }
                            EdgeKind::OutOfNode if edge.to_lane == node_lane => {
                                Some((lane_x(edge.to_lane), mid_y, oy + row_h))
                            }
                            _ => None,
                        };
                        if let Some((x, y0, y1)) = seg {
                            paint_dashed_vertical(window, x, y0, y1, color);
                            continue;
                        }
                    }

                    let mut builder = PathBuilder::stroke(theme::scaled_px(EDGE_W));

                    match edge.kind {
                        EdgeKind::Pass => {
                            let x_from = lane_x(edge.from_lane);
                            if edge.from_lane == edge.to_lane {
                                // Straight vertical line, full row height.
                                builder.move_to(point(px(x_from), px(oy)));
                                builder.line_to(point(px(x_from), px(oy + row_h)));
                            } else {
                                // Compaction shift: the lane moved column between
                                // rows — draw an S-curve from top to bottom column.
                                let x_to = lane_x(edge.to_lane);
                                draw_shift(&mut builder, x_from, oy, x_to, oy + row_h);
                            }
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
                // When the badge column has chips, draw a 1px horizontal line from
                // the left edge of the graph canvas (= right edge of badge column)
                // to the node centre. The line uses the node's lane colour.
                //
                // Skipped in swimlane mode (pad_l > 0): there the badge/spacer
                // connector already runs the label up to the lane band's left edge,
                // and the band itself carries the eye across to the avatar node — an
                // extra in-graph line just adds noise.
                if has_badges && scroll_x < 0.5 && pad_l <= 0.5 {
                    let x_node = lane_x(node_lane);
                    // Draw from the left edge of the graph area (ox) to the node.
                    // If the node is in lane 0 the line has zero length; only draw
                    // when there is meaningful horizontal distance.
                    if x_node > ox + 0.5 {
                        let color = if is_stash_lane(node_lane) {
                            stash_color
                        } else {
                            lane_color(node_color)
                        };
                        let mut builder = PathBuilder::stroke(theme::scaled_px(1.0));
                        builder.move_to(point(px(ox), px(mid_y)));
                        builder.line_to(point(px(x_node), px(mid_y)));
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, color);
                        }
                    }
                }

                // ── Draw node ● (mask clips it at the column edges) ──
                {
                    let cx_abs = lane_x(node_lane);
                    let color = if is_stash_lane(node_lane) {
                        stash_color
                    } else {
                        lane_color(node_color)
                    };

                    // W2-GRAPH: HEAD node gets a larger radius + outer ring.
                    // W2-GRAPH: merge node gets a double-circle (filled inner + stroked outer).
                    // W28: node radii scale with zoom so the ● keeps its size ratio
                    // to the (scaled) lane pitch and row height. `node_radius()` is
                    // the same helper the unit tests assert against.
                    let base_r = node_radius();
                    let head_r = base_r * 1.5; // 1.5× radius for HEAD

                    const SEGMENTS: usize = 12;

                    if is_head {
                        // HEAD: large filled circle + outer ring (same colour, slightly transparent).
                        // Outer ring (stroke).
                        let ring_r = head_r + theme::scaled(1.5);
                        let mut rb = PathBuilder::stroke(theme::scaled_px(1.2));
                        for i in 0..=SEGMENTS {
                            let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                            let px_val = cx_abs + ring_r * angle.cos();
                            let py_val = mid_y + ring_r * angle.sin();
                            if i == 0 {
                                rb.move_to(point(px(px_val), px(py_val)));
                            } else {
                                rb.line_to(point(px(px_val), px(py_val)));
                            }
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
                            let py_val = mid_y + head_r * angle.sin();
                            if i == 0 {
                                fb.move_to(point(px(px_val), px(py_val)));
                            } else {
                                fb.line_to(point(px(px_val), px(py_val)));
                            }
                        }
                        fb.close();
                        if let Ok(path) = fb.build() {
                            window.paint_path(path, color);
                        }
                    } else if is_merge {
                        // Merge: double circle — stroked outer ring + stroked inner circle.
                        // Outer ring.
                        let outer_r = base_r + theme::scaled(2.5);
                        let mut rb = PathBuilder::stroke(theme::scaled_px(1.2));
                        for i in 0..=SEGMENTS {
                            let angle = (i as f32) * 2.0 * std::f32::consts::PI / (SEGMENTS as f32);
                            let px_val = cx_abs + outer_r * angle.cos();
                            let py_val = mid_y + outer_r * angle.sin();
                            if i == 0 {
                                rb.move_to(point(px(px_val), px(py_val)));
                            } else {
                                rb.line_to(point(px(px_val), px(py_val)));
                            }
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
                            let py_val = mid_y + base_r * angle.sin();
                            if i == 0 {
                                fb.move_to(point(px(px_val), px(py_val)));
                            } else {
                                fb.line_to(point(px(px_val), px(py_val)));
                            }
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
                            let py_val = mid_y + base_r * angle.sin();
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
            }); // with_content_mask
        },
    )
}

// ──────────────────────────────────────────────────────────────
// Geometry helpers (extracted so they can be unit-tested without a Window)
// ──────────────────────────────────────────────────────────────

/// W28: x-centre of `lane` within a graph canvas whose left edge is `ox`,
/// for the current zoom and horizontal `scroll_x`.  This is exactly the
/// `lane_x` closure used inside [`graph_canvas`]'s paint pass, factored out so
/// the zoom-scaling can be asserted directly.
#[inline]
pub fn lane_center_x(ox: f32, lane: usize, scroll_x: f32) -> f32 {
    let lw = lane_w();
    ox + (lane as f32) * lw + lw / 2.0 - scroll_x
}

/// W28: vertical centre of the node ● for a canvas of measured height `row_h`
/// whose top edge is `oy`.  The node sits at the row's vertical midpoint; since
/// `row_h` is itself the zoom-scaled row height, the ● centre scales with zoom
/// automatically — this is why it stays centred at any zoom.
#[inline]
pub fn node_center_y(oy: f32, row_h: f32) -> f32 {
    oy + row_h / 2.0
}

/// W28: the zoom-scaled node radius (`NODE_R * zoom()`).
#[inline]
pub fn node_radius() -> f32 {
    theme::scaled(NODE_R)
}

// ──────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme;

    /// Approximate float equality for geometry assertions.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// W28 alignment gate. All zoom assertions live in ONE test so the global
    /// `theme::set_zoom` atomic is driven serially (cargo runs separate `#[test]`
    /// fns in parallel, which would race the shared zoom state). `KAGI_LOG_DIR`
    /// is redirected to a tempdir so `set_zoom`'s settings.json write never
    /// touches the developer's real `~/.kagi`.
    #[test]
    fn geometry_scales_uniformly_with_zoom() {
        let _g = crate::ui::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        // ── 1.0× baseline, with a NON-ZERO origin and scroll so both
        // parameters actually reach the result (expecting a restatement of the
        // function body would pass even with `ox` / `scroll_x` / `oy` dropped).
        theme::set_zoom(1.0);
        assert!(close(lane_w(), LANE_W), "lane_w @1.0 == LANE_W");
        assert!(close(node_radius(), NODE_R), "node_radius @1.0 == NODE_R");
        // ox=30, lane 2, scroll_x=12 → 30 + 2*22 + 11 - 12 = 73.
        assert!(
            close(lane_center_x(30.0, 2, 12.0), 73.0),
            "lane_center_x must add ox and subtract scroll_x, got {}",
            lane_center_x(30.0, 2, 12.0)
        );
        // oy=100, ROW_H=29 → 100 + 14.5 = 114.5.
        assert!(
            close(node_center_y(100.0, ROW_H), 114.5),
            "node_center_y must add oy, got {}",
            node_center_y(100.0, ROW_H)
        );

        // ── 0.8× : every dimension shrinks by exactly 0.8 ──────────
        theme::set_zoom(0.8);
        let z = theme::zoom();
        assert!(close(z, 0.8), "zoom set to 0.8");
        assert!(close(lane_w(), LANE_W * 0.8), "lane pitch shrinks 0.8");
        assert!(
            close(node_radius(), NODE_R * 0.8),
            "node radius shrinks 0.8"
        );
        assert!(
            close(theme::scaled(EDGE_W), EDGE_W * 0.8),
            "edge width shrinks 0.8"
        );
        assert!(
            close(theme::scaled(CORNER_R), CORNER_R * 0.8),
            "corner radius shrinks 0.8"
        );
        // Lane x-centre: lane*pitch + pitch/2, all scaled by 0.8.
        let lw08 = LANE_W * 0.8;
        assert!(close(lane_center_x(0.0, 2, 0.0), 2.0 * lw08 + lw08 / 2.0));
        // The scaled row height is what the canvas measures; ● sits at its
        // midpoint, so node_center_y of a 0.8-scaled row is 0.8 of the 1.0 one.
        let row_h_08 = f32::from(theme::scaled_px(ROW_H));
        assert!(close(node_center_y(0.0, row_h_08), ROW_H * 0.8 / 2.0));
        // lanes_for_width must agree with the (shrunk) pitch: a 112px column
        // fits 112 / (14*0.8) = 10 lanes (vs 8 at 1.0×).
        assert_eq!(lanes_for_width(112.0), (112.0 / lw08).floor() as usize);

        // ── 1.3× : every dimension grows by exactly 1.3 ────────────
        theme::set_zoom(1.3);
        let z = theme::zoom();
        assert!(close(z, 1.3), "zoom set to 1.3");
        assert!(close(lane_w(), LANE_W * 1.3), "lane pitch grows 1.3");
        assert!(close(node_radius(), NODE_R * 1.3), "node radius grows 1.3");
        assert!(
            close(theme::scaled(EDGE_W), EDGE_W * 1.3),
            "edge width grows 1.3"
        );
        let row_h_13 = f32::from(theme::scaled_px(ROW_H));
        assert!(close(node_center_y(0.0, row_h_13), ROW_H * 1.3 / 2.0));
        // A 112px column now fits fewer (wider) lanes.
        let lw13 = LANE_W * 1.3;
        assert_eq!(lanes_for_width(112.0), (112.0 / lw13).floor() as usize);

        // ── Drift check: the node centre x is always at the lane's true
        // horizontal pitch midpoint, so node↔edge endpoints share the SAME
        // lane_x() and cannot drift apart at any zoom. Verify lane N's centre
        // is exactly N pitches + half-pitch from the origin at 1.3×.
        for lane in 0..6 {
            let expect = (lane as f32) * lw13 + lw13 / 2.0;
            assert!(
                close(lane_center_x(0.0, lane, 0.0), expect),
                "lane {lane} centre at 1.3x"
            );
        }

        // Restore the default so other tests/suites see 1.0×.
        theme::set_zoom(1.0);
        std::env::remove_var("KAGI_LOG_DIR");
    }
}
