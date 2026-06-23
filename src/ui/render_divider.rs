//! Divider drag-move handler split out of `render.rs` (T-SPLIT-RENDER-001 /
//! ADR-0116 Wave 3). Child module of `crate::ui`, so it keeps direct access to
//! `KagiApp`'s private state. Behaviour is unchanged — a pure physical move
//! from `render`'s root `on_drag_move` listener.

use super::*;

impl KagiApp {
    /// T023: divider drag-move handler (single listener handles every divider).
    /// Extracted verbatim from `render` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3)
    /// so the entry `render` reads as composition; behaviour is unchanged.
    /// Placed on the root div so it fires even when the mouse moves outside the
    /// narrow 4px divider strip. Widths are derived from the ABSOLUTE cursor
    /// position, not deltas: the sidebar starts at the window's left edge and
    /// the panel ends at its right edge, so the divider simply tracks the cursor.
    pub(super) fn handle_divider_drag(
        &mut self,
        event: &gpui::DragMoveEvent<DividerDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = *event.drag(cx);
        let cursor_x = f32::from(event.event.position.x);
        // W28: sidebar/panel widths are stored UNSCALED (logical px) but
        // rendered via `scaled_px`, so the divider visually sits at
        // `width * zoom`.  The cursor is in raw window px, so convert back
        // to logical space (divide by zoom) before clamping/storing, and
        // interpret the 4px divider's 2px half-offset in scaled space too.
        let z = theme::zoom();
        match drag.kind {
            DividerKind::Sidebar => {
                // Divider sits at x = sidebar_width * zoom; centre on cursor.
                let new_width = ((cursor_x - 2.0 * z) / z).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                if (new_width - self.sidebar.width).abs() > 0.5 {
                    self.sidebar.width = new_width;
                    cx.notify();
                }
            }
            DividerKind::Panel => {
                // Divider sits at x = viewport_width - panel_width * zoom.
                let viewport_w = f32::from(window.viewport_size().width);
                let new_width = ((viewport_w - cursor_x - 2.0 * z) / z).clamp(PANEL_MIN, PANEL_MAX);
                if (new_width - self.panel_width).abs() > 0.5 {
                    self.panel_width = new_width;
                    // ADR-0117: the File History detail divider drags `panel_width`
                    // too — keep the (entity-owned) detail-pane width in sync so it
                    // resizes live.
                    if let Some(fh) = self.file_history.clone() {
                        fh.update(cx, |v, cx| {
                            v.panel_width = new_width;
                            cx.notify();
                        });
                    }
                    cx.notify();
                }
            }
            DividerKind::BadgeCol => {
                // T030/W28: badge column left edge = sidebar_width + INNER_DIV_W, all
                // rendered scaled, so the on-screen left edge is (..)*z; convert the
                // raw cursor back to logical space (/z) before clamping/storing.
                let badge_col_left = self.sidebar.width + INNER_DIV_W; // sidebar divider = 4px
                let new_w = ((cursor_x / z) - badge_col_left - INNER_DIV_W / 2.0)
                    .clamp(BADGE_COL_MIN, BADGE_COL_MAX);
                if (new_w - self.badge_col_w).abs() > 0.5 {
                    self.badge_col_w = new_w;
                    theme::set_col_width("badge_col_w", new_w);
                    cx.notify();
                }
            }
            DividerKind::GraphCol => {
                // T030/W28: graph column left edge = badge_col_left + badge_col_w + INNER_DIV_W,
                // all rendered scaled; convert the raw cursor back to logical space (/z).
                let badge_col_left = self.sidebar.width + INNER_DIV_W;
                let graph_col_left = badge_col_left + self.badge_col_w + INNER_DIV_W;
                let new_w = ((cursor_x / z) - graph_col_left - INNER_DIV_W / 2.0)
                    .clamp(GRAPH_COL_MIN, GRAPH_COL_MAX);
                if (new_w - self.graph_col_w).abs() > 0.5 {
                    self.graph_col_w = new_w;
                    theme::set_col_width("graph_col_w", new_w);
                    cx.notify();
                }
            }
            DividerKind::BottomPanel => {
                // T-BP-002: absolute-coordinate formula from ADR-0007:
                //   height = viewport_h - cursor_y - status_bar_h(22) - 2
                // W28: the panel is rendered scaled, so the on-screen gap
                // between the cursor and the window bottom is the *scaled*
                // height; divide by zoom to recover the unscaled stored
                // value. The status bar (also scaled) and divider half are
                // scaled in screen space too.
                let viewport_h = f32::from(window.viewport_size().height);
                let cursor_y = f32::from(event.event.position.y);
                // max fraction is a screen-space cap → convert to unscaled.
                let max_h = (viewport_h * BOTTOM_PANEL_MAX_FRAC) / z;
                let new_h = ((viewport_h - cursor_y - (22.0 + 2.0) * z) / z)
                    .clamp(BOTTOM_PANEL_MIN_H, max_h);
                if (new_h - self.bottom_panel_height).abs() > 0.5 {
                    self.bottom_panel_height = new_h;
                    cx.notify();
                }
            }
            DividerKind::InspectorSplit => {
                // W7-INSPECTOR2: absolute-coordinate ratio against the
                // *measured* message+files region (paint-time canvas in
                // inspector.rs).  Static offsets miss the variable-height
                // header above the region, which showed up as a ~2cm jump
                // when starting a drag.  Falls back to the constant-based
                // approximation until the first paint has run.
                let cursor_y = f32::from(event.event.position.y);
                let (geom_top, geom_bottom) = self.inspector_geom.get();
                let (top, bottom) = if geom_bottom - geom_top > 1.0 {
                    // Primary path: the canvas measured the real (already
                    // scaled) region bounds in screen px — use as-is.
                    (geom_top, geom_bottom)
                } else {
                    // Transient fallback before first paint: the layout
                    // chrome is rendered scaled, so scale the constant
                    // offsets into screen space too.
                    let viewport_h = f32::from(window.viewport_size().height);
                    let bottom_taken = if self.bottom_panel_open {
                        STATUS_BAR_H + self.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                    } else {
                        STATUS_BAR_H
                    };
                    (INSPECTOR_TOP_OFFSET * z, viewport_h - bottom_taken * z)
                };
                // The divider itself occupies INSPECTOR_SPLIT_DIVIDER_H of
                // the region; the flex split applies to the remainder. The
                // span is in screen px (scaled), so scale the divider too.
                let span = bottom - top - inspector::INSPECTOR_SPLIT_DIVIDER_H * z;
                if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                    eprintln!(
                        "[kagi] split-drag: cursor_y={:.1} top={:.1} bottom={:.1} split={:.3}",
                        cursor_y, top, bottom, self.inspector_split
                    );
                }
                if span > 1.0 {
                    let ratio =
                        ((cursor_y - top) / span).clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX);
                    if (ratio - self.inspector_split).abs() > 0.001 {
                        self.inspector_split = ratio;
                        cx.notify();
                    }
                }
            }
            DividerKind::ConflictAB => {
                // T-CONFLICT-UI-003: A|B vertical divider — ratio of the
                // measured A·B row width given to A.  The cursor sits on
                // the divider center, while flex layout assigns the ratio
                // to the space excluding the scaled divider.
                // ADR-0118: the measured `ab_geom` cell + the `ab_split` live on
                // the `ConflictView` entity now; read the shared cell and push the
                // new ratio in via `entity.update` (mirrors FileHistory).
                let cursor_x = f32::from(event.event.position.x);
                let Some(entity) = self.conflict.clone() else {
                    return;
                };
                let (left, right) = entity.read(cx).ab_geom.get();
                if let Some(ratio) = conflict_split_ratio_from_cursor(
                    cursor_x,
                    left,
                    right,
                    CONFLICT_SPLIT_DIVIDER * z,
                    CONFLICT_AB_MIN,
                    CONFLICT_AB_MAX,
                ) {
                    entity.update(cx, |v, cx| v.set_ab_split(ratio, cx));
                }
            }
            DividerKind::FileHistoryRows => {
                // ADR-0089: list/diff vertical split. Use the region's
                // *measured* (top, bottom) screen bounds recorded by the
                // paint-time canvas in `render_fh_list_and_diff`, so the
                // cursor maps exactly. Falls back to a constant offset
                // until the first paint has run.
                let cursor_y = f32::from(event.event.position.y);
                let (geom_top, geom_bottom) = self.file_history_geom.get();
                let (top, bottom) = if geom_bottom - geom_top > 1.0 {
                    (geom_top, geom_bottom)
                } else {
                    let viewport_h = f32::from(window.viewport_size().height);
                    let bottom_taken = if self.bottom_panel_open {
                        STATUS_BAR_H + self.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                    } else {
                        STATUS_BAR_H
                    };
                    (110.0 * z, viewport_h - bottom_taken * z)
                };
                let span = bottom - top;
                if span > 1.0 {
                    // ADR-0117: split lives in the entity; mutate it via `update`
                    // (its `set_split` applies the 0.002 threshold + child notify).
                    let ratio = ((cursor_y - top) / span).clamp(0.15, 0.85);
                    if let Some(fh) = self.file_history.clone() {
                        fh.update(cx, |v, cx| v.set_split(ratio, cx));
                    }
                }
            }
            DividerKind::ConflictResult => {
                // T-CONFLICT-UI-003: A·B / Result horizontal divider — ratio
                // of the measured editor split region given to the A·B row.
                // The previous separate hunk-control strip is gone; chunk
                // controls live inside the A/B lists, so this measured
                // region now matches the rendered split exactly.
                // ADR-0118: `geom` cell + `result_split` live on the entity now.
                let cursor_y = f32::from(event.event.position.y);
                let Some(entity) = self.conflict.clone() else {
                    return;
                };
                let (top, bottom) = entity.read(cx).geom.get();
                if let Some(ratio) = conflict_split_ratio_from_cursor(
                    cursor_y,
                    top,
                    bottom,
                    CONFLICT_SPLIT_DIVIDER * z,
                    CONFLICT_RESULT_MIN,
                    CONFLICT_RESULT_MAX,
                ) {
                    entity.update(cx, |v, cx| v.set_result_split(ratio, cx));
                }
            }
        }
    }
}
