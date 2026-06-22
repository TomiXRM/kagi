//! WIP rows and stash-graph rows split out of `render.rs`
//! (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). These row builders are consumed by
//! `render_body`. Child module of `crate::ui`, so direct access to `KagiApp`'s
//! private state is preserved. Behaviour is unchanged — a pure physical move.

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
use super::*;

impl KagiApp {
    /// Render the stash graph rows (ADR-0088): one fixed row per stash, shown
    /// directly below the WIP row, in the stash colour with an inbox icon and a
    /// graph node that connects down to the stash's base commit. Left-click pops,
    /// right-click opens the stash menu (same as the sidebar).
    pub(super) fn render_stash_graph_rows(
        &self,
        badge_col_w: f32,
        graph_col_w: f32,
        graph_scroll_x: f32,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let visible_lanes = graph_view::lanes_for_width(graph_col_w);
        let stash_color = theme().color_warning;
        let stash_lanes = self.active_view.stash_graph_lanes.clone();
        let rh = row_height(self.graph_compact);

        // Lanes of connected stashes rendered *above* the current row, whose
        // branch lines must keep passing straight down through this row (fixes
        // the topmost stash's line vanishing at the next stash row).
        let mut passing_lanes: Vec<usize> = Vec::new();

        self.active_view
            .stash_graph_rows
            .iter()
            .map(|sr| {
                let index = sr.index;
                let label = sr.label.clone();
                let msg_for_menu = sr.label.to_string();
                let mut edges: Vec<kagi::graph::GraphEdge> = passing_lanes
                    .iter()
                    .map(|&lane| kagi::graph::GraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        kind: kagi::graph::EdgeKind::Pass,
                        // Stash lanes are painted in the stash colour; the lane
                        // colour index is unused for them.
                        color: lane,
                    })
                    .collect();
                if sr.connected {
                    // This stash's own line leaves its node downward; below this
                    // row it becomes a pass-through for subsequent rows.
                    edges.push(kagi::graph::GraphEdge {
                        from_lane: sr.lane,
                        to_lane: sr.lane,
                        kind: kagi::graph::EdgeKind::OutOfNode,
                        color: sr.lane,
                    });
                    passing_lanes.push(sr.lane);
                }
                let pop = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                    this.open_pop_modal(index);
                    cx.notify();
                });
                let menu = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
                    this.open_stash_menu(index, msg_for_menu.clone(), e.position);
                    cx.stop_propagation();
                    cx.notify();
                });
                let (cb, cbd, ct) = theme::badge_style(stash_color);
                div()
                    .id(("stash-graph-row", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .px_3()
                    .h(px(rh))
                    .on_click(pop)
                    .on_mouse_down(gpui::MouseButton::Right, menu)
                    .hover(|s| s.bg(rgb(theme().selected)))
                    // Badge column: a yellow stash chip with an inbox icon.
                    .child(
                        div()
                            .w(theme::scaled_px(badge_col_w))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_start()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(gpui::rgba(cb))
                                    .border_1()
                                    .border_color(gpui::rgba(cbd))
                                    .text_color(rgb(ct))
                                    .text_sm()
                                    .child(
                                        gpui::svg()
                                            .path("icons/inbox.svg")
                                            .w(theme::scaled_px(12.))
                                            .h(theme::scaled_px(12.))
                                            .text_color(rgb(ct)),
                                    )
                                    .child(SharedString::from("stash")),
                            )
                            // Connector line into the BRANCH/TAG pane toward the
                            // stash node (only when it connects to a base).
                            .when(sr.connected, |el| {
                                el.child(div().flex_1().h_full().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Inner divider spacer (badge|graph), bridged for the connector.
                    .child(
                        div()
                            .relative()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface)))
                            .when(sr.connected, |el| {
                                el.child(div().absolute().inset_0().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Graph column: the stash node + line down to its base.
                    .child(
                        div()
                            .w(theme::scaled_px(graph_col_w))
                            .h_full()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .when(visible_lanes > 0, |el| {
                                el.child(
                                    graph_view::graph_canvas(
                                        sr.lane,
                                        // Stash nodes paint in the stash colour;
                                        // node_color is unused for them.
                                        sr.lane,
                                        edges,
                                        visible_lanes,
                                        false,
                                        false,
                                        true,
                                        graph_scroll_x,
                                        graph_lane_pad_l(),
                                        stash_lanes.clone(),
                                    )
                                    .size_full(),
                                )
                            }),
                    )
                    // Inner divider spacer (graph|message).
                    .child(
                        div()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                    )
                    // Message column: the stash label, in the stash colour.
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .truncate()
                            .text_color(rgb(stash_color))
                            .child(label),
                    )
                    .into_any()
            })
            .collect()
    }

    /// Render one WIP row for a single worktree (Model A+: every worktree's
    /// uncommitted state is shown at once, each tinted in its own colour so the
    /// rows are distinguishable at a glance — user request).
    ///
    /// The currently-open worktree's row opens the commit panel (stage/unstage)
    /// and carries a live `+/-` diffstat; a linked worktree's row switches the
    /// open repo to that worktree so its changes can be acted on there.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_wip_row(
        &self,
        color: gpui::Hsla,
        label: SharedString,
        change_count: usize,
        diffstat: Option<WipDiffStat>,
        click: WipRowClick,
        is_worktree: bool,
        commit_panel_open: bool,
        badge_col_w: f32,
        graph_col_w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let is_commit_panel = matches!(click, WipRowClick::CommitPanel);
        let dark = theme().dark;
        // Tinted chip built from the worktree's lane colour (badge_style only
        // takes a packed u32; lane_color is Hsla, so build the tint directly).
        // Stronger fill than a ref badge so the WIP rows read loudly.
        let chip_bg = gpui::hsla(color.h, color.s, color.l, if dark { 0.32 } else { 1.0 });
        let chip_border = gpui::hsla(color.h, color.s, color.l, if dark { 0.60 } else { 1.0 });
        let chip_text = if dark {
            rgb(0xffffff)
        } else {
            rgb(theme().bg_base)
        };
        // Whole-row background: a subtle wash of the worktree colour so each WIP
        // row is distinguishable at a glance (user request), with a bold colour
        // stripe down the left edge for prominence.
        let row_wash = gpui::hsla(color.h, color.s, color.l, if dark { 0.14 } else { 0.10 });

        let note = if is_commit_panel {
            i18n::wip_row_note(change_count)
        } else {
            i18n::wip_row_other(change_count)
        };

        // Glyph distinguishes the row's working tree: 🌲 for a linked worktree
        // (ties to its 🌲 HEAD badge in the graph), ✏️ for the main repo's
        // normal branch (just an editable working tree, not a worktree).
        let glyph = if is_worktree { "🌲" } else { "✏️" };
        let chip_label = SharedString::from(format!("{glyph} {label}"));

        let mut row = div()
            .id(SharedString::from(format!("wip-row-{label}")))
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .pr_3()
            .border_l(theme::scaled_px(3.))
            .border_color(color)
            .pl(theme::scaled_px(9.))
            .h(px(row_height(self.graph_compact)));
        row = if is_commit_panel && commit_panel_open {
            row.bg(rgb(theme().selected))
        } else {
            row.bg(row_wash)
        };
        // Both row kinds are clickable: the open repo's row opens the commit
        // panel; a linked worktree's row switches the open repo to it. The two
        // closures are distinct types, so wire `on_click` inside each match arm.
        row = match click {
            WipRowClick::CommitPanel => {
                row.on_click(cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    this.open_commit_panel(window, cx);
                    cx.notify();
                }))
            }
            WipRowClick::OpenWorktree(path) => row.on_click(cx.listener(
                move |this, _e: &gpui::ClickEvent, _window, cx| {
                    this.open_repository(path.clone(), cx);
                    cx.notify();
                },
            )),
        };
        row = row.hover(|s| s.bg(rgb(theme().selected))).cursor_pointer();

        row
            // Badges column: worktree-coloured chip carrying the glyph + the branch
            // name, left-aligned like the commit-row badges.
            .child(
                div()
                    .w(theme::scaled_px(badge_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_start()
                    .gap_1()
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(chip_bg)
                            .border_1()
                            .border_color(chip_border)
                            .text_color(chip_text)
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .flex_shrink_0()
                            .overflow_hidden()
                            .truncate()
                            .child(chip_label),
                    ),
            )
            // Inner divider spacer (badge|graph handle width)
            .child(
                div()
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
            )
            // Graph column: hollow "not yet committed" node tinted in the
            // worktree colour — visually continues the graph upward.
            .child(
                div()
                    .w(theme::scaled_px(graph_col_w))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .ml(theme::scaled_px(graph_view::LANE_W / 2.0 - 4.5))
                            .w(theme::scaled_px(9.))
                            .h(theme::scaled_px(9.))
                            .rounded_full()
                            .border_1()
                            .border_color(color),
                    ),
            )
            // Inner divider spacer (graph|message handle width)
            .child(
                div()
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
            )
            // Summary area: change-count note, with the +N/-M diffstat (when
            // available, i.e. the current worktree) pushed to the right end.
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(theme().text_muted))
                            .overflow_hidden()
                            .truncate()
                            .child(SharedString::from(note)),
                    )
                    .when_some(diffstat, |el, stat| el.child(render_wip_diffstat(stat))),
            )
            .into_any_element()
    }
}
