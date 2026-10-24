//! Divider drag-move handler split out of `render.rs` (T-SPLIT-RENDER-001 /
//! ADR-0116 Wave 3). Child module of `crate::ui`, so it keeps direct access to
//! `KagiApp`'s private state. The root `on_drag_move` listener handles pane and
//! Graph header dividers; Graph columns use measured painted origins (#1011).

use super::*;

impl KagiApp {
    pub(super) fn finish_bottom_panel_resize(&mut self) {
        if let Some((_, _, original_height)) = self.bottom_panel_drag_origin.take() {
            if !self.compact.enabled && (self.bottom_panel_height - original_height).abs() > 0.5 {
                settings::write_setting(
                    "bottom_panel_height",
                    Some(&self.bottom_panel_height.to_string()),
                );
            }
        }
    }

    /// T023: divider drag-move handler (single listener handles every divider).
    /// On the root div so a drag continues outside the narrow divider strip.
    /// Widths use absolute pointer positions, not deltas: outer panes anchor to
    /// window edges, while Graph columns anchor to their painted header bounds.
    pub(super) fn handle_divider_drag(
        &mut self,
        event: &gpui::DragMoveEvent<DividerDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = *event.drag(cx);
        let cursor_x = f32::from(event.event.position.x);
        // W28: pane widths are stored in reference-space logical px but
        // rendered through `scaled_px`; undo the full physical scale (the
        // 14.4px rem base and relative zoom) for raw cursor coordinates.
        // The 4px divider's 2px half-offset follows that same scale.
        let z = theme::scaled(1.0);
        // #957: sidebar and outer-pane dividers stay put during a slide.
        // Graph column dividers use their painted column origins instead,
        // so they can follow the pointer while the sidebar is moving (#1011).
        let now = kagi_ui_core::motion::now();
        let sliding = match drag.kind {
            DividerKind::Sidebar => self.panel_motion.left.animating(now),
            DividerKind::Panel => self.panel_motion.right.animating(now),
            DividerKind::BottomPanel => self.panel_motion.bottom.animating(now),
            _ => false,
        };
        if sliding {
            return;
        }
        match drag.kind {
            DividerKind::Sidebar | DividerKind::PrModeLeft => {
                // Divider sits at x = sidebar_width * physical scale.
                let new_width = ((cursor_x - 2.0 * z) / z).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                if matches!(drag.kind, DividerKind::Sidebar) {
                    let candidate = self.pane_fit.refit(new_width, self.panel_width);
                    if (candidate.sidebar - self.pane_fit.sidebar).abs() <= 0.5 {
                        return;
                    }
                }
                if (new_width - self.sidebar.width).abs() > 0.5 {
                    self.sidebar.width = new_width;
                    cx.notify();
                }
            }
            DividerKind::SidebarPane(index) => {
                if self.sidebar.resize_pane_pair(
                    usize::from(index),
                    f32::from(event.event.position.y),
                    z,
                ) {
                    cx.notify();
                }
            }
            DividerKind::Panel => {
                // Divider sits at x = viewport_width - panel_width * physical scale.
                let viewport_w = f32::from(window.viewport_size().width);
                let new_width = ((viewport_w - cursor_x - 2.0 * z) / z).clamp(PANEL_MIN, PANEL_MAX);
                // File History owns a nested divider; only the outer right
                // slot shares this budget. Ignore already-capped expansions.
                if self.panel_motion.right_pane.is_some() {
                    let candidate = self.pane_fit.refit(self.sidebar.width, new_width);
                    if (candidate.right - self.pane_fit.right).abs() <= 0.5 {
                        return;
                    }
                }
                if (new_width - self.panel_width).abs() > 0.5 {
                    self.panel_width = new_width;
                    // ADR-0117: the File History detail divider drags `panel_width`
                    // too — keep the (entity-owned) detail-pane width in sync so it
                    // resizes live.
                    if let Some(fh) = self.ui().file_history.clone() {
                        fh.update(cx, |v, cx| {
                            v.panel_width = new_width;
                            cx.notify();
                        });
                    }
                    cx.notify();
                }
            }
            DividerKind::BadgeCol => {
                // The painted column origin includes sidebar visibility, motion,
                // and header padding; only the divider's half-width is added.
                let Some(left) = self.badge_col_origin_x.get() else {
                    return;
                };
                let new_w =
                    ((cursor_x - left) / z - INNER_DIV_W / 2.0).clamp(BADGE_COL_MIN, BADGE_COL_MAX);
                // A capped expansion must not replace the larger saved preference
                // with a candidate measured from its compressed painted origin.
                let candidate = self.graph_column_fit.refit(new_w, self.graph_col_w);
                if (candidate.badge - self.graph_column_fit.badge).abs() <= 0.5 {
                    return;
                }
                if (new_w - self.badge_col_w).abs() > 0.5 {
                    self.badge_col_w = new_w;
                    theme::set_col_width("badge_col_w", new_w);
                    cx.notify();
                }
            }
            DividerKind::GraphCol => {
                let Some(left) = self.graph_col_origin_x.get() else {
                    return;
                };
                let new_w =
                    ((cursor_x - left) / z - INNER_DIV_W / 2.0).clamp(GRAPH_COL_MIN, GRAPH_COL_MAX);
                let candidate = self.graph_column_fit.refit(self.badge_col_w, new_w);
                if (candidate.graph - self.graph_column_fit.graph).abs() <= 0.5 {
                    return;
                }
                if (new_w - self.graph_col_w).abs() > 0.5 {
                    self.graph_col_w = new_w;
                    theme::set_col_width("graph_col_w", new_w);
                    cx.notify();
                }
            }
            DividerKind::BottomPanel => {
                // Start from the actual pressed body height: the rendered panel
                // already includes its divider and tab strip. Applying the
                // pointer delta also preserves an off-centre grab, and keeping
                // the origin immutable prevents drift after clamp overshoot.
                let Some((origin_y, origin_height, preferred_height)) =
                    self.bottom_panel_drag_origin
                else {
                    return;
                };
                let cursor_y = f32::from(event.event.position.y);
                let max_h = super::render_bottom::max_body_height(
                    window,
                    self.workspace_mode() == workspace_mode::WorkspaceMode::Editor,
                );
                let new_h =
                    (origin_height + (origin_y - cursor_y) / z).clamp(BOTTOM_PANEL_MIN_H, max_h);
                // Returning to the press position restores a preference that
                // was larger than its fitted presentation; a stationary frame
                // must never convert that cap into a new preference.
                let new_h = if (cursor_y - origin_y).abs() <= 0.5 * z {
                    preferred_height
                } else {
                    new_h
                };
                if self.compact.enabled {
                    if self
                        .compact
                        .bottom_height
                        .is_none_or(|height| (new_h - height).abs() > 0.5)
                    {
                        self.compact.bottom_height = Some(new_h);
                        cx.notify();
                    }
                    return;
                }
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
                    // The inspector now extends beside the center-only
                    // bottom panel; only the window-wide status bar is below.
                    let viewport_h = f32::from(window.viewport_size().height);
                    (INSPECTOR_TOP_OFFSET * z, viewport_h - STATUS_BAR_H * z)
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
                let Some(entity) = self.ui().conflict.clone() else {
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
                    if let Some(fh) = self.ui().file_history.clone() {
                        fh.update(cx, |v, cx| v.set_split(ratio, cx));
                    }
                }
            }
            DividerKind::EditorTree => {
                // Fit and input share the actual Editor body, whose left edge
                // can sit after the host's Compact rail.
                let Some(entity) = self.ui().editor_workspace.clone() else {
                    return;
                };
                entity.update(cx, |v, cx| {
                    if !v.show_tree || v.pane_widths_animating() {
                        return;
                    }
                    let bounds = v.pane_bounds(window);
                    let new_w = v
                        .pane_width_from_rendered(
                            (cursor_x - f32::from(bounds.left()) - 2.0 * z) / z,
                            bounds.size.width,
                        )
                        .clamp(EDITOR_TREE_MIN, EDITOR_TREE_MAX);
                    if (new_w - v.tree_w).abs() > 0.5 {
                        v.tree_w = new_w;
                        cx.notify();
                    }
                });
            }
            DividerKind::EditorHunks => {
                // The hunks divider sits at the pane's LEFT edge; dragging
                // right shrinks it, measured from the Editor body's right edge.
                let Some(entity) = self.ui().editor_workspace.clone() else {
                    return;
                };
                entity.update(cx, |v, cx| {
                    if !v.details_resizing_available() {
                        return;
                    }
                    let bounds = v.pane_bounds(window);
                    let new_w = v
                        .pane_width_from_rendered(
                            (f32::from(bounds.right()) - cursor_x - 2.0 * z) / z,
                            bounds.size.width,
                        )
                        .clamp(EDITOR_HUNKS_MIN, EDITOR_HUNKS_MAX);
                    if (new_w - v.hunks_w).abs() > 0.5 {
                        v.hunks_w = new_w;
                        cx.notify();
                    }
                });
            }
            DividerKind::CleanupCol(idx) => {
                // ADR-0128: Branch Cleanup table columns start after the
                // sidebar (when visible) + its divider; convert the raw
                // cursor to pane-relative logical px and let the pane's own
                // handler do the per-column offset math.
                let pane_left = if self.sidebar.visible {
                    self.pane_fit.sidebar + INNER_DIV_W
                } else {
                    0.0
                };
                self.handle_cleanup_col_drag(idx, cursor_x / z - pane_left, cx);
            }
        }
    }
}
