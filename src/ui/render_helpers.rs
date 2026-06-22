//! Render helper functions extracted from render.rs (ADR-0113 / Phase D).
//!
//! These are the pure-data-in / element-out builders for the GPUI view tree.
//! They take view-model data (CommitRow, MainDiffView, CommitPanelState, etc.)
//! and return GPUI elements. None are methods on KagiApp (no `self` receiver).
//!
//! Extracted from render.rs to bring it under the 800-LOC target (AGENTS.md).
//! render.rs retains only the `impl Render` + view-construction methods.

#![allow(clippy::too_many_arguments)]

use super::*;
use gpui_component::button::{Button, ButtonVariants};

// T-SPLIT-HELPERS-001 / ADR-0116 Wave 3: the commit-panel, badge, and file-menu
// renderers moved to focused sibling modules. Re-export them here so the existing
// `use super::render_helpers::*;` call sites keep resolving without touching the
// render_*.rs callers (public paths preserved).
// ADR-0117: the file-history renderers are now internal to `file_history_render`
// (the `Entity<FileHistoryView>` renders itself), so they are no longer
// re-exported here.
pub(crate) use super::badges::*;
pub(crate) use super::file_menu::*;

/// Left pad (px) applied to the graph lane geometry in swimlane mode so lane 0
/// clears the column's left edge (room for the avatar node). 0 in classic mode.
/// Must match between the main rows and the stash rows so their lanes line up.
pub(crate) fn graph_lane_pad_l() -> f32 {
    if theme::graph_lane_compact() {
        theme::scaled(8.0)
    } else {
        0.0
    }
}

/// A solid 1px horizontal connector line (branch label -> graph node / lane band
/// edge), filling its parent's width.
pub(crate) fn connector_line(color: gpui::Hsla) -> gpui::Div {
    div().w_full().h(theme::scaled_px(1.)).bg(color)
}

pub(crate) fn render_rows(
    rows: &[CommitRow],
    avatar_images: &HashMap<String, std::sync::Arc<gpui::Image>>,
    range: std::ops::Range<usize>,
    selected: Option<usize>,
    badge_col_w: f32,
    graph_col_w: f32,
    graph_compact: bool,
    graph_scroll_x: f32,
    stash_lanes: &[usize],
    solo_visible: Option<&HashSet<CommitId>>,
    cx: &mut Context<KagiApp>,
) -> Vec<impl IntoElement> {
    let rh = row_height(graph_compact);

    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            // T-PERF-RENDER-002 (ADR-0116 Wave 2): `row` stays a `&CommitRow`
            // borrowed from `rows`; the click/context handlers capture `ix`
            // (not the row), and every field read either copies a `Copy` value
            // or bumps an Arc (`SharedString`) / clones a small Vec, so the
            // whole-row clone per visible row is unnecessary.
            let is_selected = selected == Some(ix);
            let is_dimmed = solo_visible.is_some_and(|visible| !visible.contains(&row.id));

            // Selected row gets a prominent surface highlight;
            // even/odd stripes apply otherwise.
            let row_bg = if is_selected {
                theme().selected
            } else if ix % 2 == 0 {
                theme().bg_base
            } else {
                theme().bg_row_alt
            };
            // Swimlane lane band (Gitru-style): an inner horizontal strip drawn
            // BEHIND the graph column only and tinted by this row's lane colour.
            // It is a separate absolute layer — NOT the row background — with
            // vertical inset so adjacent bands don't touch and the row stays a
            // neutral list. The message column is intentionally left neutral so
            // text stays readable. Gated on lane-compaction; skipped on the
            // selected row so the selection highlight always wins. Tuple =
            // (normal, hover) wash colours.
            let lane_band: Option<(gpui::Hsla, gpui::Hsla)> =
                if theme::graph_lane_compact() && !is_selected {
                    let c = theme().lane_color(row.node_color);
                    let (na, ha) = if theme().dark {
                        (0.18, 0.24)
                    } else {
                        (0.11, 0.15)
                    };
                    Some((gpui::hsla(c.h, c.s, c.l, na), gpui::hsla(c.h, c.s, c.l, ha)))
                } else {
                    None
                };
            // Lane-driven ref-pill colour: every pill on this commit uses the
            // commit's lane colour so pills agree with the graph line / node /
            // band. Always on (independent of the avatar-nodes toggle), matching
            // the lane-coloured graph. Per-ref lanes aren't modelled, so the
            // primary commit lane is the documented fallback.
            let pill_lane: Option<u32> = Some(theme::lane_color_u32(row.node_color));

            // ── Graph lane area (T030) ────────────────────────
            // visible_lanes = how many lanes fit in the current graph column width.
            // This replaces the old MAX_LANES-based clipping.
            let visible_lanes = graph_view::lanes_for_width(graph_col_w);

            // on_click handler: update KagiApp.selected via cx.listener.
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.commit_menu = None;
                this.select(ix);
                // ADR-0089 Phase 2c: kick off the remote changed-files load now
                // that we have `cx` (the render trigger is a fallback for
                // keyboard navigation). Idempotent.
                if this.remote_view.is_some() && this.selected == Some(ix) {
                    this.load_remote_changed_files(ix, cx);
                }
                cx.notify();
            });
            let context_click_handler =
                cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.open_commit_menu(ix, event.position);
                    cx.stop_propagation();
                    cx.notify();
                });

            // ── Avatar (T020 / W11-AVATAR) ────────────────────
            let avatar_color = avatar::avatar_color(&row.author_email);
            let avatar_init = SharedString::from(avatar::avatar_initial(&row.author));
            // Convert Hsla to the rgb u32 that gpui's `bg()` accepts via hsla().
            let av_bg = avatar_color;
            // W11-AVATAR: real GitHub avatar if resolved, else initial circle.
            let avatar_image = avatar_images.get(&row.author_email).cloned();

            // W2-GRAPH: badge presence flag for label→node connector line.
            let has_badges = !row.badges.is_empty();
            // Connector colour for the badge→node line (extends into the
            // BRANCH/TAG pane). Matches the node's lane colour; only when the
            // graph isn't scrolled sideways (the canvas connector is gated the
            // same way).
            let connector_color: Option<gpui::Hsla> = if has_badges && graph_scroll_x < 0.5 {
                // Use the node's stable lane colour (matches the graph node / band
                // / pills) rather than the bare lane index.
                Some(theme().lane_color(row.node_color))
            } else {
                None
            };

            // Swimlane: render the author avatar AS the commit node inside the
            // graph (ringed in the lane colour, Gitru-style), replacing the
            // separate avatar in the message column. Only when the node's lane is
            // within the visible (horizontally-scrolled) lane window so it isn't
            // clipped to a wrong position.
            let lane_w_px = graph_view::lane_w();
            // Left pad inside the graph: shifts the lanes + avatar node right so
            // lane 0's avatar sits fully inside the column with a sliver of the
            // lane band visible to its left (the band itself stays at the column
            // edge). Passed into the canvas (lane geometry) and used for the node
            // x; 0 in classic mode.
            let graph_pad_l = graph_lane_pad_l();
            let lane_lo = (graph_scroll_x / lane_w_px).floor().max(0.0) as usize;
            let avatar_in_graph = theme::graph_lane_compact()
                && visible_lanes > 0
                && row.lane >= lane_lo
                && row.lane < lane_lo + visible_lanes;
            // Local x-centre of the node within the graph column (incl. left pad).
            let node_cx =
                graph_pad_l + (row.lane as f32) * lane_w_px + lane_w_px / 2.0 - graph_scroll_x;
            let avatar_image_g = avatar_image.clone();
            let avatar_init_g = avatar_init.clone();

            div()
                .id(ix)
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                // W2-GRAPH item 3: 2px accent bar on the left edge of selected
                // rows. Drawn as an ABSOLUTE overlay (the child below) so
                // selection does NOT change the row's horizontal layout — both
                // states use the same `px_3()`, keeping the graph column origin
                // (and thus the graph lanes) aligned across selected and
                // unselected rows at every zoom level.
                //
                // Previously the selected row used `pl(scaled_px(12) - 2) +
                // border_l_2`, whose left inset scales with zoom while `px_3`
                // does NOT (gpui 0.2.2 resolves rem-size for text, not for this
                // padding — see theme.rs scaled_px notes). After the rem-size
                // gating in T-PERF-RENDER-002 that mismatch became visible: at
                // zoom > 100% the selected row's graph lane drifted right, at
                // zoom < 100% it drifted left, off the unselected rows' lanes.
                .px_3()
                .when(is_selected, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(rgb(theme().color_branch)),
                    )
                })
                .h(px(rh))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                .on_mouse_down(MouseButton::Right, context_click_handler)
                .when(is_dimmed, |el| el.opacity(0.32))
                // ── Badges column: user-resizable width (T030) ──
                // T-DNDMERGE-001: thread `cx` so each `BadgeKind::Branch` chip
                // can be made draggable and the HeadBranch chip a drop target.
                // Reborrow `cx` (the `.map()` closure already mutably borrows it
                // for `cx.listener(...)` above) per row.
                .child(render_badges_column(
                    &row.id,
                    &row.badges,
                    badge_col_w,
                    connector_color,
                    pill_lane,
                    &mut *cx,
                ))
                // ── Inner divider spacer (badge|graph handle width) ──
                // When the row has a badge connector, bridge the 4px gap with a
                // horizontal line so the badge→node connector stays continuous.
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
                        .when_some(connector_color, |el, color| {
                            // Fill height + items_center so the 1px line is
                            // centred exactly like the badge-column and canvas
                            // connectors (no 1px step at the boundary).
                            el.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .child(connector_line(color)),
                            )
                        }),
                )
                // ── Graph lane area (T030) ────────────────────────
                // Always render the graph column at graph_col_w width.
                // Clip by visible_lanes to prevent bleed into message column.
                .child(
                    div()
                        .relative()
                        .w(theme::scaled_px(graph_col_w))
                        .h_full()
                        .flex_shrink_0()
                        // ── Lane band (swimlane) ──
                        // A child of the GRAPH column (not the row) so it is
                        // contained to the column and cannot reach into the
                        // BRANCH/TAG label area. Absolute, inset 0 horizontally
                        // (= the column width) with a 3px top/bottom inset so
                        // adjacent bands don't touch; painted before the canvas
                        // so it sits behind the lanes/nodes.
                        .when_some(lane_band, |el, (band, band_hover)| {
                            el.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(theme::scaled_px(3.))
                                    .bottom(theme::scaled_px(3.))
                                    .rounded(theme::scaled_px(3.))
                                    .bg(band)
                                    .hover(|s| s.bg(band_hover)),
                            )
                        })
                        // Clip to the column so nothing reaches the BRANCH/TAG or
                        // message columns; the avatar fits thanks to graph_pad_l.
                        .overflow_hidden()
                        // Horizontal wheel/trackpad scroll reveals clipped
                        // lanes. Vertical deltas are left untouched so the
                        // commit list keeps scrolling normally.
                        .on_scroll_wheel(cx.listener(
                            move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                                this.scroll_graph_by(&e.delta, cx);
                            },
                        ))
                        .when(visible_lanes > 0, |el| {
                            // The left pad is applied inside the canvas (lane
                            // geometry) via `graph_pad_l`, NOT as a `pl` here, so
                            // the label→node connector still starts at the column
                            // edge and doesn't gap.
                            el.child(
                                div().size_full().child(
                                    graph_canvas(
                                        row.lane,
                                        row.node_color,
                                        row.edges.clone(),
                                        visible_lanes,
                                        row.is_head,
                                        row.is_merge,
                                        has_badges,
                                        graph_scroll_x,
                                        graph_pad_l,
                                        stash_lanes.to_vec(),
                                    )
                                    .size_full(),
                                ),
                            )
                        })
                        // Swimlane: avatar node, drawn over the canvas at the
                        // node centre with a lane-colour ring (the coloured disc
                        // shows as a ~1.5px ring around the inner avatar).
                        .when(avatar_in_graph, |el| {
                            let ring_d = theme::scaled(18.);
                            let av_d = theme::scaled(15.);
                            let inner = div()
                                .w(px(av_d))
                                .h(px(av_d))
                                .rounded_full()
                                .overflow_hidden();
                            let inner = match avatar_image_g {
                                Some(image) => inner.child(
                                    gpui::img(gpui::ImageSource::Image(image))
                                        .size_full()
                                        .rounded_full(),
                                ),
                                None => inner
                                    .bg(av_bg)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .text_color(gpui::white())
                                            .text_xs()
                                            .child(avatar_init_g),
                                    ),
                            };
                            el.child(
                                div()
                                    .absolute()
                                    .left(px(node_cx - ring_d / 2.))
                                    .top(px(rh / 2. - ring_d / 2.))
                                    .w(px(ring_d))
                                    .h(px(ring_d))
                                    .rounded_full()
                                    .bg(theme().lane_color(row.node_color))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(inner),
                            )
                        }),
                )
                // ── Inner divider spacer (graph|message handle width) ──
                .child(
                    div()
                        .w(theme::scaled_px(INNER_DIV_W))
                        .flex_shrink_0()
                        .flex()
                        .justify_center()
                        .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                )
                // ── Author avatar: 18px circle after graph ────────
                // W11-AVATAR: when a GitHub avatar is resolved, show the image
                // clipped to the circle; otherwise the initial-on-colour circle.
                // Skipped in swimlane mode — the avatar is drawn as the graph
                // node instead (Gitru-style), so the message column starts with
                // the summary text.
                .when(!avatar_in_graph, |row_el| {
                    row_el.child({
                        // W28: avatar circle scales with zoom so it stays sized to
                        // the (rem-scaled) row text and aligned with the graph node.
                        let circle = div()
                            .w(theme::scaled_px(18.))
                            .h(theme::scaled_px(18.))
                            .flex_shrink_0()
                            .mr(theme::scaled_px(4.))
                            .rounded_full()
                            .overflow_hidden();
                        match avatar_image {
                            Some(image) => circle.child(
                                gpui::img(gpui::ImageSource::Image(image))
                                    .size_full()
                                    .rounded_full(),
                            ),
                            None => circle
                                .bg(av_bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div().text_color(gpui::white()).text_xs().child(avatar_init),
                                ),
                        }
                    })
                })
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(theme().text_main))
                        // Single line, no wrapping: long summaries ellipsize
                        // (truncate = overflow_hidden + nowrap + ellipsis).
                        .truncate()
                        .child(row.summary.clone()),
                )
                .child(
                    // W28: author/date columns scale so the (rem-scaled) text
                    // fits its box at any zoom.
                    div()
                        .w(theme::scaled_px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_sub))
                        .truncate()
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .w(theme::scaled_px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_muted))
                        .child(row.date.clone()),
                )
        })
        .collect()
}

/// The synthetic trailing row of the commit list: a centred, clickable
/// "load more commits" affordance shown only when the graph may have been
/// truncated by `commit_limit`. Clicking it grows the walk by
/// [`COMMIT_PAGE_STEP`](crate::ui::COMMIT_PAGE_STEP) via
/// [`KagiApp::load_more_commits`]. Height matches a commit row so the
/// `uniform_list` stays uniform.
pub(crate) fn render_load_more_row(
    graph_compact: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let rh = row_height(graph_compact);
    div()
        .id("commit-load-more")
        .h(px(rh))
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_xs()
        .text_color(rgb(theme().color_branch))
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.load_more_commits(cx);
        }))
        .child(SharedString::from(
            crate::ui::i18n::Msg::LoadMoreCommits.t(),
        ))
        .into_any_element()
}

// Note: render_detail_panel was extracted to src/ui/inspector.rs (W2-INSPECTOR).

// ──────────────────────────────────────────────────────────────
// T-UI-003: Main pane diff renderer (full-width)
// ──────────────────────────────────────────────────────────────

/// Render the full-width main pane diff view.
///
/// Layout (fills remaining width after sidebar + divider):
/// - Header row: `← Back` + file name + stats
/// - Body: `uniform_list` id `"main-diff-list"` with line numbers
/// W6-TABSPEED / ADR-0030: center-pane placeholder shown while an uncached tab
/// is loading on a background thread.  The tab strip stays operable above it.
pub(crate) fn render_loading_placeholder(label: SharedString) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .text_lg()
                .text_color(rgb(theme().text_sub))
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("\u{27f3}")), // ⟳
        )
}

/// ADR-0117: the diff "list body" (header line + virtualized rows + scrollbar),
/// parameterized over the entity context `V` so it can be rendered from either
/// `KagiApp` (the standalone main diff) or `FileHistoryView` (its embedded diff
/// pane). The `uniform_list` row processor ignores the entity (`_this`), so this
/// works for any `V: 'static`. `leading` / `trailing` are the optional standalone
/// header buttons (Back / History) — `None` when embedded in File History, which
/// supplies its own Back. Never read an entity back via `cx` here — this runs
/// during render (the rendering entity is already borrowed → panic).
pub(crate) fn render_diff_list<V: 'static>(
    view: MainDiffView,
    leading: Option<gpui::AnyElement>,
    trailing: Option<gpui::AnyElement>,
    scroll_handle: UniformListScrollHandle,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let row_count = view.rows.len();
    let title = view.title.clone();
    let stats = view.stats.clone();
    let rows = std::sync::Arc::new(view.rows);
    let rows_for_list = rows.clone();

    div()
        .flex_1()
        // Allow the center diff column to shrink below its content's intrinsic
        // width so the right-hand inspector panel keeps its space. Flex
        // min-width defaults to content size, which for a wide diff (long lines)
        // or a widened inspector pushes the `flex_shrink_0` panel off the right
        // edge — its diffstat badges / Tree button then overflow off-screen
        // (user report: widening the commit pane while viewing a file diff).
        // Mirrors the same fix on `commit_list_col` in render.rs.
        .min_w(px(0.))
        .overflow_hidden()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        // ── Header row (fixed height) ─────────────────────────────────────
        .child(
            div()
                .id("main-diff-header")
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .gap_2()
                .bg(rgb(theme().surface))
                // ← Back button (only for the standalone main diff; the File
                // History view embeds this diff and has its own Back).
                .when_some(leading, |el, btn| el.child(btn))
                // File name
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .truncate()
                        .child(title),
                )
                // History button (导线 #3)
                .when_some(trailing, |el, btn| el.child(btn))
                // Stats: +N −M
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .flex_shrink_0()
                        .child(stats),
                ),
        )
        // ── Diff body: full remaining space ──────────────────────────────
        .child({
            // W12-GCADOPT (§2.10): Scrollbar overlay on the diff list.
            let scrollbar_handle = scroll_handle.clone();
            with_vertical_scrollbar(
                "main-diff-list-scroll",
                &scrollbar_handle,
                uniform_list(
                    "main-diff-list",
                    row_count,
                    cx.processor(move |_this: &mut V, range, _window, _cx| {
                        render_main_diff_rows(&rows_for_list, range)
                    }),
                )
                .track_scroll(scroll_handle)
                .flex_1()
                .min_h(px(0.)),
                true,
            )
        })
}

/// Standalone main diff view (KagiApp): the embedded diff list plus the
/// KagiApp-bound header buttons (← Back / History). The File History view
/// renders its diff pane via [`render_diff_list`] directly (no buttons).
pub(crate) fn render_main_diff_view(
    view: MainDiffView,
    scroll_handle: UniformListScrollHandle,
    // Standalone main diff (true) vs reused inside the File History view
    // (false). When embedded in File History, the header's Back and History
    // buttons are hidden — the File History view has its own Back.
    standalone: bool,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let (leading, trailing): (Option<gpui::AnyElement>, Option<gpui::AnyElement>) = if standalone {
        // "← Back" click handler: close the main diff view.
        let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.close_main_diff();
            cx.notify();
        });
        let history_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.open_file_history_from_main_diff(cx);
            cx.notify();
        });
        (
            Some(
                Button::new("main-diff-back")
                    .label("\u{2190} Back")
                    .ghost()
                    .small()
                    .on_click(back_click)
                    .into_any_element(),
            ),
            Some(
                Button::new("main-diff-history")
                    .label("History")
                    .ghost()
                    .small()
                    .flex_shrink_0()
                    .on_click(history_click)
                    .into_any_element(),
            ),
        )
    } else {
        (None, None)
    };

    render_diff_list::<KagiApp>(view, leading, trailing, scroll_handle, cx)
}

/// W12-GCADOPT (§2.10): wrap a virtualized list in a relative flex column and
/// overlay a `gpui_component::scroll::Scrollbar` driven by the list's existing
/// `UniformListScrollHandle`.  The Scrollbar paints itself absolutely-positioned
/// over the container (relative(1.) size), so this is layout-non-destructive —
/// the inner `uniform_list` keeps its own `flex_1().min_h(0)` sizing.  Colours
/// follow the gpui-component scrollbar theme fields, which
/// `sync_gpui_component_theme` keeps in step with kagi's palette.
/// `show_bar` controls whether the overlay scrollbar is rendered. `false` hides
/// it entirely (the list still scrolls via wheel/trackpad) — used for the commit
/// stage/unstage lists, which the user wants free of a visible scrollbar. When
/// `true` the bar follows the theme default (`cx.theme().scrollbar_show`, which
/// honours the macOS "show scroll bars" setting).
pub(crate) fn with_vertical_scrollbar(
    id: &'static str,
    handle: &UniformListScrollHandle,
    list: impl IntoElement,
    show_bar: bool,
) -> impl IntoElement {
    let mut container = div()
        .id(id)
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(list);
    if show_bar {
        container = container.child(Scrollbar::vertical(handle));
    }
    container
}
