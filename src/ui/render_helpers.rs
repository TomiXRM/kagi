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
use crate::ui::button_style::KagiButton;
use gpui_component::button::{Button, ButtonVariants};

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
fn connector_line(color: gpui::Hsla) -> gpui::Div {
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
                // W2-GRAPH item 3: 2px accent bar on the left edge of selected rows.
                // We use pl_3() normally and reduce the inner padding by 2px when
                // selected to make room for the bar without changing total row width.
                .when(is_selected, |el| {
                    // W28: non-selected rows use px_3 (0.75rem) which scales with
                    // zoom; the selected row must match so the graph column origin
                    // doesn't shift horizontally on selection. Left inset =
                    // scaled px_3 minus the fixed 2px accent bar.
                    el.pl(theme::scaled_px(12.) - px(2.))
                        .border_l_2()
                        .border_color(rgb(theme().color_branch))
                })
                .when(!is_selected, |el| el.px_3())
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

pub(crate) fn render_main_diff_view(
    view: MainDiffView,
    scroll_handle: UniformListScrollHandle,
    // Standalone main diff (true) vs reused inside the File History view
    // (false). When embedded in File History, the header's Back and History
    // buttons are hidden — the File History view has its own Back. Passed in by
    // the caller — never read the KagiApp entity here, since this runs during
    // render (the entity is already borrowed → panic).
    standalone: bool,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let row_count = view.rows.len();
    let rows = std::sync::Arc::new(view.rows);
    let rows_for_list = rows.clone();
    let title = view.title.clone();
    let stats = view.stats.clone();

    // "← Back" click handler: close the main diff view.
    let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.close_main_diff();
        cx.notify();
    });
    let history_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.open_file_history_from_main_diff(cx);
        cx.notify();
    });

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
                .when(standalone, |el| {
                    el.child(
                        Button::new("main-diff-back")
                            .label("\u{2190} Back")
                            .ghost()
                            .small()
                            .on_click(back_click),
                    )
                })
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
                .when(standalone, |el| {
                    el.child(
                        Button::new("main-diff-history")
                            .label("History")
                            .ghost()
                            .small()
                            .flex_shrink_0()
                            .on_click(history_click),
                    )
                })
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
                    cx.processor(move |_this, range, _window, _cx| {
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

// ──────────────────────────────────────────────────────────────
// ADR-0089: File History view rendering
// ──────────────────────────────────────────────────────────────

/// A small text "chip" button used in the File History header.
pub(crate) fn fh_header_button(
    id: &'static str,
    label: impl Into<SharedString>,
    on_click: impl Fn(&mut KagiApp, &gpui::ClickEvent, &mut gpui::Window, &mut Context<KagiApp>)
        + 'static,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    Button::new(id)
        .label(label.into())
        .ghost()
        .small()
        .on_click(cx.listener(on_click))
}

/// Render the entire File History view (center + right detail pane), ADR-0089.
///
/// Reuses [`render_main_diff_view`] for the diff body.  Returns the body
/// fragment that `render_body` drops in place of the normal center+right area.
pub(crate) fn render_file_history_view(
    state: &file_history::FileHistoryState,
    file_history_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
    fh_branch: SharedString,
    panel_width: f32,
    geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Extract the scalar/owned view data from the shared `state` borrow. Render
    // functions must NEVER read the entity back via `cx` — that re-enters the
    // already checked-out KagiApp entity and panics. State is from render_body.
    let (rel_path, follow, split, count, is_loading, error, is_empty, is_untracked) = (
        state.rel_path.clone(),
        state.follow_renames,
        state.split,
        state.commit_count(),
        state.is_loading(),
        state.error.clone(),
        state.is_empty(),
        state.is_untracked(),
    );
    let rel_path_str = SharedString::from(rel_path.to_string_lossy().into_owned());

    // ── Header ──────────────────────────────────────────────────────
    let back = fh_header_button(
        "fh-back",
        "\u{2190} Back",
        |this, _e, _w, cx| {
            this.close_file_history();
            cx.notify();
        },
        cx,
    );

    let path_for_copy = rel_path.clone();
    let copy_path = fh_header_button(
        "fh-copy-path",
        "Copy Path",
        move |_this, _e, _w, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                path_for_copy.to_string_lossy().into_owned(),
            ));
        },
        cx,
    );

    let refresh = fh_header_button(
        "fh-refresh",
        "Refresh",
        |this, _e, _w, cx| {
            this.refresh_file_history(cx);
        },
        cx,
    );

    let path_for_open = rel_path.clone();
    let open_file = fh_header_button(
        "fh-open-file",
        "Open File",
        move |this, _e, _w, _cx| {
            // v1: return to the normal body; the file's diff is reachable via
            // the commit panel / inspector.  Keep it simple per the spec.
            let _ = &path_for_open;
            this.close_file_history();
        },
        cx,
    );

    let follow_label = if follow {
        "Follow Renames: On"
    } else {
        "Follow Renames: Off"
    };
    let follow_btn = fh_header_button(
        "fh-follow",
        follow_label,
        |this, _e, _w, cx| {
            this.toggle_file_history_follow(cx);
        },
        cx,
    );

    let header = div()
        .id("fh-header")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .px_3()
        .py_1()
        .gap_2()
        .bg(rgb(theme().surface))
        .child(back)
        .child(
            div()
                .id("fh-title")
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(SharedString::from(format!(
                    "File History: {}",
                    rel_path_str
                )))
                .tooltip(move |window, cx| Tooltip::new(rel_path_str.clone()).build(window, cx)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .child(fh_branch.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{} commits", count))),
        )
        .child(refresh)
        .child(copy_path)
        .child(open_file)
        .child(follow_btn);

    // ── Center column (list + diff) selection of the body content ──
    let center_body: gpui::AnyElement = if is_loading {
        render_fh_message("Loading file history...", false, cx).into_any_element()
    } else if let Some(err) = error {
        render_fh_error(err, cx).into_any_element()
    } else if is_empty {
        render_fh_message("No history found for this file.", false, cx).into_any_element()
    } else if is_untracked {
        // Untracked: show the message but still allow the WIP diff below.
        render_fh_list_and_diff(
            state,
            split,
            Some("This file is untracked. No commit history yet."),
            geom,
            cx,
        )
        .into_any_element()
    } else {
        render_fh_list_and_diff(state, split, None, geom, cx).into_any_element()
    };

    let center = div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_w(px(0.))
        .bg(rgb(theme().panel))
        .child(header)
        .child(center_body);

    // ── Right detail pane ──────────────────────────────────────────
    let detail_divider = div()
        .id("fh-detail-divider")
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::Panel,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let detail_pane = render_fh_detail_pane(state, panel_width, cx);

    // ── Optional row context menu overlay ──────────────────────────
    let menu_overlay = file_history_menu.map(|(ix, pos)| render_fh_row_menu(state, ix, pos, cx));

    div()
        .id("file-history-view")
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .child(center)
        .child(detail_divider)
        .child(detail_pane)
        .children(menu_overlay)
        .into_any_element()
}

/// A centered single-line message (Loading / Empty), optionally an error tint.
pub(crate) fn render_fh_message(
    msg: &'static str,
    is_error: bool,
    _cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let color = if is_error {
        theme().color_blocker
    } else {
        theme().text_muted
    };
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(color))
        .child(SharedString::from(msg))
}

/// Error state: message + detail + Retry button.
pub(crate) fn render_fh_error(detail: String, cx: &mut Context<KagiApp>) -> impl IntoElement {
    let retry = div()
        .id("fh-retry")
        .px_3()
        .py_1()
        .rounded_sm()
        .bg(rgb(theme().bg_base))
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .on_click(cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.refresh_file_history(cx);
        }))
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .child(SharedString::from("Retry"));

    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .child(SharedString::from("Failed to load file history.")),
        )
        .child(
            div()
                .max_w(px(520.))
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(detail)),
        )
        .child(retry)
}

/// The vertically-split commit list (top) + diff viewer (bottom).
pub(crate) fn render_fh_list_and_diff(
    state: &file_history::FileHistoryState,
    split: f32,
    banner: Option<&'static str>,
    geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let list = render_fh_commit_list(state, cx);
    let (diff_view, diff_scroll, sel_banner) = {
        let diff = state.diff.clone();
        let scroll = state.diff_scroll.clone();
        // Per-entry banner (Added / Deleted / Renamed) above the diff.
        let sel_banner = state.selected_entry().map(|e| {
            use kagi_git::FileChangeType;
            match e.change.change_type {
                FileChangeType::Added => "This file was added in this commit.".to_string(),
                FileChangeType::Deleted => "This file was deleted in this commit.".to_string(),
                FileChangeType::Renamed => {
                    let before = e
                        .change
                        .path_before
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let after = e.change.path_after.to_string_lossy().into_owned();
                    format!("{} \u{2192} {}", before, after)
                }
                _ if e.change.is_binary => {
                    "Binary file changed. Preview is not available.".to_string()
                }
                _ => String::new(),
            }
        });
        (diff, scroll, sel_banner)
    };

    // Divider between list and diff (horizontal drag).
    let h_divider = div()
        .id("fh-rows-divider")
        .w_full()
        .h(theme::scaled_px(4.))
        .flex_shrink_0()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
        .cursor_row_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::FileHistoryRows,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let list_frac = split.clamp(0.15, 0.85);
    let diff_frac = 1.0 - list_frac;

    let diff_section = div()
        .w_full()
        .flex()
        .flex_col()
        .flex_grow()
        .flex_basis(gpui::relative(diff_frac))
        .min_h(px(0.))
        // Optional view-level banner (untracked note).
        .when_some(banner, |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().color_warning))
                    .child(SharedString::from(b)),
            )
        })
        // Per-entry banner (added/deleted/renamed/binary).
        .when_some(sel_banner.filter(|s| !s.is_empty()), |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .bg(rgb(theme().bg_row_alt))
                    .child(SharedString::from(b)),
            )
        })
        .child(match diff_view {
            Some(view) => render_main_diff_view(view, diff_scroll, false, cx).into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("No diff available for this entry."))
                .into_any_element(),
        });

    // Paint-time canvas records the real (top, bottom) screen bounds of this
    // list+diff region so the divider drag maps the cursor exactly (a constant
    // top offset misses the variable-height header → the drag would jump).
    let measure = {
        let geom = geom.clone();
        gpui::canvas(
            move |_b: gpui::Bounds<gpui::Pixels>, _w: &mut Window, _cx: &mut gpui::App| {},
            move |b: gpui::Bounds<gpui::Pixels>, _p: (), _w: &mut Window, _cx: &mut gpui::App| {
                let top = f32::from(b.origin.y);
                geom.set((top, top + f32::from(b.size.height)));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };

    div()
        .relative()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(measure)
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .flex_grow()
                .flex_basis(gpui::relative(list_frac))
                .min_h(px(0.))
                .child(list),
        )
        .child(h_divider)
        .child(diff_section)
}

/// The commit list (upper pane) of the File History view.
pub(crate) fn render_fh_commit_list(
    state: &file_history::FileHistoryState,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let Some(history) = state.history.as_ref() else {
        return div().into_any_element();
    };
    let entries = &history.entries;
    let selected = state.selected;
    let now = commit_list::now_unix_secs();

    let mut list = div()
        .id("fh-commit-list")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .min_h(px(0.));

    for (ix, entry) in entries.iter().enumerate() {
        list = list.child(render_fh_row(ix, entry, ix == selected, now, cx));
    }

    list.into_any_element()
}

/// One row in the File History commit list.
pub(crate) fn render_fh_row(
    ix: usize,
    entry: &kagi_git::FileHistoryEntry,
    is_selected: bool,
    now: i64,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    use kagi_git::FileHistoryEntryKind;

    let (badge, badge_color) = file_history::entry_badge(entry);
    let is_wip = entry.kind == FileHistoryEntryKind::Wip;

    let (subject, author, date, short_hash) = if is_wip {
        (
            SharedString::from("WIP \u{2014} Uncommitted changes"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    } else if let Some(c) = entry.commit.as_ref() {
        let date = file_history::iso_to_epoch(&c.author_date)
            .map(|e| commit_list::relative_time(e, now))
            .unwrap_or_default();
        (
            SharedString::from(c.subject.clone()),
            SharedString::from(c.author_name.clone()),
            SharedString::from(date),
            SharedString::from(c.short_hash.clone()),
        )
    } else {
        (
            SharedString::from("(unknown)"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    };

    let ins = entry.change.insertions;
    let del = entry.change.deletions;
    let stat = if entry.change.is_binary {
        SharedString::from("bin")
    } else {
        SharedString::from(format!(
            "+{} \u{2212}{}",
            ins.unwrap_or(0),
            del.unwrap_or(0)
        ))
    };

    let row_bg = if is_selected {
        theme().selected
    } else if ix % 2 == 1 {
        theme().bg_row_alt
    } else {
        theme().panel
    };

    let click = cx.listener(move |this, e: &gpui::ClickEvent, _w, cx| {
        this.file_history_menu = None;
        if e.click_count() >= 2 {
            // Double-click: jump to the commit in the graph (commits only).
            if let Some(id) = this
                .file_history
                .as_ref()
                .and_then(|fh| fh.history.as_ref())
                .and_then(|h| h.entries.get(ix))
                .and_then(|e| e.commit.as_ref())
                .map(|c| CommitId(c.full_hash.clone()))
            {
                this.close_file_history();
                this.jump_to_commit(&id);
                cx.notify();
                return;
            }
        }
        this.file_history_select(ix, cx);
    });
    let ctx = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
        this.file_history_menu = Some((ix, e.position));
        cx.stop_propagation();
        cx.notify();
    });

    div()
        .id(("fh-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .px_3()
        .py_px()
        .h(px(row_height(false)))
        .flex_shrink_0()
        .bg(rgb(row_bg))
        .on_click(click)
        .on_mouse_down(MouseButton::Right, ctx)
        .cursor_pointer()
        // Hover uses the subtle `surface` tint (like the commit panel / branch
        // list), NOT `selected` — using the selection colour made a hovered row
        // indistinguishable from the selected one, so the row the mouse was left
        // on after a click looked "still selected" while the arrows moved the
        // real selection elsewhere. The selected row keeps its colour on hover.
        .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        // change-type letter
        .child(
            div()
                .w(theme::scaled_px(18.))
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        // subject
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(subject),
        )
        // author
        .child(
            div()
                .w(theme::scaled_px(90.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(author),
        )
        // relative date
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(date),
        )
        // +ins / -del
        .child(
            div()
                .w(theme::scaled_px(72.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(stat),
        )
        // short hash
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(short_hash),
        )
}

/// Right detail pane for the selected File History entry.
pub(crate) fn render_fh_detail_pane(
    state: &file_history::FileHistoryState,
    panel_width: f32,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Clone the entry out so listeners can capture owned data.
    let entry: Option<kagi_git::FileHistoryEntry> = state.selected_entry().cloned();

    let mut pane = div()
        .id("fh-detail-pane")
        .w(theme::scaled_px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .bg(rgb(theme().panel))
        .overflow_y_scroll();

    let Some(entry) = entry else {
        return pane
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("No entry selected.")),
            )
            .into_any_element();
    };

    let line = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(value)),
            )
    };

    let ct = entry.change.change_type;
    let ct_label = file_history::change_type_label(ct).to_string();
    let stat = if entry.change.is_binary {
        "binary".to_string()
    } else {
        format!(
            "+{} \u{2212}{}",
            entry.change.insertions.unwrap_or(0),
            entry.change.deletions.unwrap_or(0)
        )
    };
    let path_after = entry.change.path_after.to_string_lossy().into_owned();
    let path_before = entry
        .change
        .path_before
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    if let Some(c) = entry.commit.as_ref() {
        let full = c.full_hash.clone();
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(c.subject.clone())),
            )
            .child(line("Full Hash", c.full_hash.clone()))
            .child(line("Short Hash", c.short_hash.clone()));

        if let Some(body) = c.body.as_ref() {
            pane = pane.child(line("Message", body.clone()));
        }
        pane = pane
            .child(line(
                "Author",
                format!("{} <{}>", c.author_name, c.author_email),
            ))
            .child(line("Committer", c.committer_name.clone()))
            .child(line("Author Date", c.author_date.clone()))
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path After", path_after));
        if let Some(before) = path_before {
            pane = pane.child(line("Path Before", before));
        }

        // ── Actions ──
        let id_open = CommitId(full.clone());
        let id_graph = CommitId(full.clone());
        let full_for_copy = full.clone();
        let actions = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .mt_2()
            .child(fh_header_button(
                "fh-detail-open",
                "Open Commit",
                move |this, _e, _w, cx| {
                    this.close_file_history();
                    this.jump_to_commit(&id_open);
                    cx.notify();
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-graph",
                "Show in Graph",
                move |this, _e, _w, cx| {
                    this.close_file_history();
                    this.jump_to_commit(&id_graph);
                    cx.notify();
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-copy",
                "Copy Hash",
                move |_this, _e, _w, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(full_for_copy.clone()));
                },
                cx,
            ));
        pane = pane.child(actions);
    } else {
        // WIP entry — minimal detail.
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from("Uncommitted changes")),
            )
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path", path_after));
    }

    pane.into_any_element()
}

/// Context menu for a File History commit row (ADR-0089).
pub(crate) fn render_fh_row_menu(
    state: &file_history::FileHistoryState,
    ix: usize,
    pos: gpui::Point<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Resolve the entry's data up front (commit hash + path at this commit).
    let (commit_hash, path_at) = {
        let entry = state.history.as_ref().and_then(|h| h.entries.get(ix));
        let commit_hash = entry
            .and_then(|e| e.commit.as_ref())
            .map(|c| c.full_hash.clone());
        let path_at = entry.map(|e| e.change.path_after.to_string_lossy().into_owned());
        (commit_hash, path_at)
    };

    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _w, cx| {
        this.file_history_menu = None;
        cx.notify();
    });

    fn item<F>(id: &'static str, label: &'static str, on_click: F) -> gpui::Stateful<gpui::Div>
    where
        F: Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    {
        div()
            .id(id)
            .px_3()
            .py(theme::scaled_px(3.))
            .text_sm()
            .text_color(rgb(theme().text_main))
            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
            .on_click(on_click)
            .child(SharedString::from(label))
    }

    let mut menu = div()
        .absolute()
        .left(pos.x)
        .top(pos.y)
        .w(theme::scaled_px(220.))
        .occlude()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .rounded_md()
        .shadow_lg()
        .py(theme::scaled_px(2.));

    if let Some(hash) = commit_hash.clone() {
        let h1 = hash.clone();
        menu = menu.child(item(
            "fh-menu-copy-hash",
            "Copy Commit Hash",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.file_history_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(h1.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(p) = path_at.clone() {
        menu = menu.child(item(
            "fh-menu-copy-path",
            "Copy File Path at This Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.file_history_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(hash) = commit_hash.clone() {
        let id_open = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-open-commit",
            "Open Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.file_history_menu = None;
                this.close_file_history();
                this.jump_to_commit(&id_open);
                cx.notify();
            }),
        ));
        let id_graph = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-graph",
            "Show Commit in Graph",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.file_history_menu = None;
                this.close_file_history();
                this.jump_to_commit(&id_graph);
                cx.notify();
            }),
        ));
    }

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(menu)
        .into_any_element()
}

/// Render the badge chips for one commit row as a horizontal flex container.
///
/// Badge labels are capped at 24 visible chars with a trailing `…` to prevent
/// very long branch names from overflowing the commit list row (T019).
/// Sort key for badge priority: HeadBranch=0, Branch=1, Tag=2, Remote=3.
/// Right-aligned layout means the last-rendered badge is closest to the graph,
/// so we want the most important badge last → highest priority rendered last.
/// We render in priority order (0→3) so HeadBranch ends up leftmost and
/// Remote rightmost within the 150px column (closest to the graph).
pub(crate) fn badge_priority(kind: &BadgeKind) -> u8 {
    match kind {
        BadgeKind::HeadBranch => 0,
        BadgeKind::Branch => 1,
        BadgeKind::Tag => 2,
        BadgeKind::Remote => 3,
    }
}

/// What clicking a WIP row does.
pub(crate) enum WipRowClick {
    /// Open the commit panel for the currently-open repo (stage/unstage).
    CommitPanel,
    /// Switch the open repo to this linked worktree so its changes can be acted
    /// on there (the open repo's WIP row, in turn, opens the commit panel).
    OpenWorktree(std::path::PathBuf),
}

pub(crate) fn render_wip_diffstat(stat: WipDiffStat) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .text_sm()
        .font_weight(gpui::FontWeight::BOLD)
        .child(
            div()
                .text_color(rgb(theme().change_added))
                .child(SharedString::from(format!("+{}", stat.additions))),
        )
        .child(
            div()
                .text_color(rgb(theme().change_deleted))
                .child(SharedString::from(format!("-{}", stat.deletions))),
        )
}

/// Render the badges column: user-resizable width (T030), **left-aligned**
/// (user request), `overflow_hidden`.  An empty badges list still occupies
/// the full width so that all rows share the same graph start position
/// (GitKraken layout, T021).  `badge_col_w` is the current column width.
pub(crate) fn render_badges_column(
    row_id: &CommitId,
    badges: &[commit_list::RefBadge],
    badge_col_w: f32,
    // When `Some`, draw a horizontal connector line filling the space between
    // the badges and the right edge of the column, so the badge→node line is
    // continuous *inside* the BRANCH/TAG pane (not stopping at the boundary).
    connector_color: Option<gpui::Hsla>,
    // Swimlane mode: when `Some`, every pill uses this lane colour (`0xRRGGBB`)
    // instead of its semantic HEAD/branch/remote/tag colour, so pills agree with
    // the graph line / node / band. `None` = classic semantic colours.
    lane_pill_color: Option<u32>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Content is built to fit rather than relying on clipping:
    //   - left-aligned, so the highest-priority chip (leftmost) is always
    //     fully visible and overflow happens rightward — the direction
    //     gpui's overflow_hidden actually clips,
    //   - the "+N" chip sits right after the primary chip so it can't be
    //     clipped,
    //   - the secondary chip flex-shrinks with an ellipsis; only its already
    //     ellipsized tail can ever be cut off.
    const MAX_BADGES: usize = 2;
    const MAX_BADGE_CHARS: usize = 20;

    let mut by_prio: Vec<&commit_list::RefBadge> = badges.iter().collect();
    by_prio.sort_by_key(|b| badge_priority(&b.kind));
    let extra = by_prio.len().saturating_sub(MAX_BADGES);
    let shown = &by_prio[..by_prio.len().min(MAX_BADGES)];

    let mut inner = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .gap_1()
        .overflow_hidden();

    // Badges in priority order: primary (HEAD/branch) leftmost.
    for (i, badge) in shown.iter().enumerate() {
        let target = row_id.clone();
        let color = match lane_pill_color {
            Some(c) => c,
            None => match badge.kind {
                BadgeKind::HeadBranch => theme().color_head,
                BadgeKind::Branch => theme().color_branch,
                BadgeKind::Remote => theme().color_remote,
                BadgeKind::Tag => theme().color_tag,
            },
        };
        // Char-truncate long labels.
        let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
            let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
            SharedString::from(format!("{}\u{2026}", s))
        } else {
            badge.label.clone()
        };
        let is_primary = i == 0;
        let (badge_bg, badge_border, badge_text) = theme::badge_style(color);
        let chip = div()
            // Stable element id so gpui interactivity (drag/drop) works. Keyed
            // by row position + badge label so a row with multiple branch chips
            // gets distinct ids (a commit can carry several branches).
            .id(SharedString::from(format!(
                "graph-badge-{i}-{}",
                badge.label
            )))
            .px_1()
            .rounded_sm()
            .bg(gpui::rgba(badge_bg))
            .border_1()
            .border_color(gpui::rgba(badge_border))
            .text_color(rgb(badge_text))
            .text_sm()
            .when(is_primary, |c| c.flex_shrink_0())
            // Secondary chips may shrink to fit; their text ellipsizes.
            .when(!is_primary, |c| c.min_w(px(20.)).truncate())
            .child(label);

        // T-DNDMERGE-001 / ADR-0079: wire drag/drop onto the chip based on kind.
        //   - `BadgeKind::Branch` / `BadgeKind::Remote` → INDEPENDENTLY draggable,
        //     carrying ITS OWN name (= the merge source) in `BranchDrag { name }`.
        //     For a remote chip the name is the full `remote/name` ref, so an
        //     upstream-only branch can be merged directly. Each visible chip
        //     carries its own name, so dragging a specific badge unambiguously
        //     selects that branch even when a commit has several. Tag chips are
        //     NOT draggable.
        //   - `BadgeKind::HeadBranch` (the current branch) → drop TARGET. It
        //     shows a valid-target highlight via `.drag_over::<BranchDrag>` and
        //     dispatches to `start_merge_from_drag` on drop. The drop is a
        //     TRIGGER only — it never calls git from the view (same as sidebar).
        let chip = match badge.kind {
            BadgeKind::Branch | BadgeKind::Remote => {
                if let Some(name) = draggable_branch_name(badge) {
                    chip.cursor_grab().on_drag(
                        BranchDrag { name: name.clone() },
                        move |drag: &BranchDrag, _pos, _window, cx| {
                            let name = SharedString::from(drag.name.clone());
                            cx.new(|_| BranchDragGhost { name })
                        },
                    )
                } else {
                    chip
                }
            }
            BadgeKind::HeadBranch => {
                let drop_handler = cx.listener(
                    move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                        this.start_merge_from_drag(payload.name.clone(), cx);
                        cx.notify();
                    },
                );
                chip.drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                    style
                        .bg(rgb(theme().selected))
                        .border_color(rgb(theme().color_branch))
                })
                .on_drop::<BranchDrag>(drop_handler)
            }
            BadgeKind::Tag => chip,
        };
        // Double-click a branch pill → switch. A local-branch pill checks out
        // the branch; a remote-branch pill switches to its latest (create/
        // fast-forward the tracking branch). A clean plan switches with no
        // popup; blockers/warnings open the relevant modal (see
        // `dblclick_checkout_branch` / `dblclick_switch_to_latest`). The
        // current-branch (HeadBranch) and tags are unaffected. Uses the full
        // `badge.label` (the displayed `label` may be truncated).
        let chip = match badge.kind {
            BadgeKind::Branch => {
                let dbl_branch = badge.label.to_string();
                chip.on_click(cx.listener(
                    move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.dblclick_checkout_branch(dbl_branch.clone(), cx);
                            cx.notify();
                        }
                    },
                ))
            }
            BadgeKind::Remote => {
                let dbl_remote = badge.label.to_string();
                chip.on_click(cx.listener(
                    move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.dblclick_switch_to_latest(dbl_remote.clone(), cx);
                            cx.notify();
                        }
                    },
                ))
            }
            BadgeKind::HeadBranch | BadgeKind::Tag => chip,
        };
        let chip = if let Some(branch_name) = context_branch_name(badge) {
            let badge_kind = badge.kind.clone();
            chip.on_mouse_down(
                MouseButton::Right,
                cx.listener(
                    move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                        match badge_kind {
                            BadgeKind::HeadBranch | BadgeKind::Branch => {
                                this.open_local_branch_menu(branch_name.clone(), event.position);
                            }
                            BadgeKind::Remote => {
                                this.open_remote_branch_menu(
                                    branch_name.clone(),
                                    target.clone(),
                                    event.position,
                                );
                            }
                            BadgeKind::Tag => {}
                        }
                        cx.stop_propagation();
                        cx.notify();
                    },
                ),
            )
        } else {
            chip
        };
        inner = inner.child(chip);

        // "+N" chip directly after the primary chip (never clipped).
        // TODO(T-DNDMERGE-001): badges hidden behind the "+N" overflow are not
        // individually draggable yet (only the up-to-MAX_BADGES visible chips
        // are). Redesigning the overflow into a draggable popover is out of
        // scope for this lane.
        if is_primary && extra > 0 {
            inner = inner.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_color(rgb(theme().text_sub))
                    .text_sm()
                    .flex_shrink_0()
                    .child(SharedString::from(format!("+{extra}"))),
            );
        }
    }

    // User-resizable container (T030), overflow clipped so long badge lists don't push graph.
    div()
        .w(theme::scaled_px(badge_col_w))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .child(inner)
        // Connector line: fills the remaining width up to the column's right
        // edge so the line reaches into the BRANCH/TAG pane toward the badge.
        .when_some(connector_color, |el, color| {
            el.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(connector_line(color)),
            )
        })
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

/// Unstaged file-row context menu (right-click). Single item: Discard.
///
/// Only attached to eligible rows (tracked, non-conflicted), so the item is
/// always actionable. Backdrop click dismisses; backdrop AND card `.occlude()`
/// (click-through bug).
pub(crate) fn render_file_menu_overlay(
    fi: usize,
    pos: gpui::Point<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _window, cx| {
        this.file_menu = None;
        cx.notify();
    });
    let discard_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.file_menu = None;
        this.open_discard_modal_for_index(fi);
        cx.notify();
    });
    // ADR-0089: open File History for this unstaged file.
    let history_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.file_menu = None;
        if let Some(path) = this
            .commit_panel
            .as_ref()
            .and_then(|p| p.unstaged.get(fi))
            .map(|f| f.path.clone())
        {
            this.open_file_history(path, None, cx);
        }
        cx.notify();
    });
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(
            div()
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(theme::scaled_px(190.))
                .occlude()
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(theme().surface))
                .rounded_md()
                .shadow_lg()
                // W27-UIPOLISH: compact (Zed-style) density — tighter vertical
                // padding to match the commit/branch context menus.
                .py(theme::scaled_px(2.))
                .child(
                    div()
                        .id(("file-menu-history", fi))
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(history_click)
                        .child(SharedString::from("Show File History")),
                )
                .child(
                    div()
                        .id(("file-menu-discard", fi))
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(discard_click)
                        .child(SharedString::from("Discard changes…")),
                ),
        )
        .into_any_element()
}

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from
// `this.commit_panel` (NOT a captured-by-value clone).  They are invoked from
// the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// PERF: recompute the WIP-highlight target from the open main diff.
/// `Some((staged, path))` when a WIP (unstaged/staged) file is open in the
/// center diff; mirrors the value the old call site passed in by value.
pub(crate) fn cp_active_wip(this: &KagiApp) -> Option<(bool, PathBuf)> {
    match this.main_diff.as_ref().map(|d| &d.source) {
        Some(MainDiffSource::Unstaged { path }) => Some((false, path.clone())),
        Some(MainDiffSource::Staged { path }) => Some((true, path.clone())),
        _ => None,
    }
}

/// PERF: build one unstaged row in flat view (index `fi` into `unstaged`).
pub(crate) fn render_unstaged_flat_row(
    this: &KagiApp,
    fi: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let f = panel.unstaged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let is_conflicted_file = panel.is_conflicted(&f.path);
    let (badge, badge_color, _) = status_badge(&f.change, is_conflicted_file);
    let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
    let stat = panel.unstaged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| !*st && &f.path == p);

    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
        cx.notify();
    });
    let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.do_stage_file(fi);
        cx.notify();
    });
    // Row background: conflicted files get red tint
    let row_bg = if is_conflicted_file {
        theme().diff_removed_bg
    } else if is_sel {
        theme().selected
    } else {
        theme().panel
    };
    let mut file_row = div()
        .id(("cp-us-flat-file", fi))
        .when(wip_hit, |el| el.bg(rgb(theme().selected)))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_px()
        .bg(rgb(row_bg))
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(file_click)
        .child(
            div()
                .w(theme::scaled_px(12.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_main))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(name)),
        )
        .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
    // Stage button only for non-conflicted files
    if !is_conflicted_file {
        // W17-DISCARD / ADR-0083: right-click opens the file context menu
        // (Discard lives there). Tracked rows are restored from the index;
        // untracked rows are deleted (after an ODB backup).
        let menu_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _window, cx| {
            this.file_menu = Some((fi, e.position));
            cx.stop_propagation();
            cx.notify();
        });
        file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
        file_row = file_row.child(
            KagiButton::accent(
                ("cp-us-flat-stage-btn", fi),
                "Stage",
                theme().color_success,
                cx,
            )
            .xsmall()
            .ml_2()
            .flex_shrink_0()
            .on_click(stage_click),
        );
    } else {
        file_row = file_row.child(
            div()
                .id(("cp-us-flat-conflict-badge", fi))
                .ml_2()
                .px_1()
                .py_px()
                .rounded_sm()
                .flex_shrink_0()
                .bg(rgb(theme().color_blocker)) // red
                .text_xs()
                .text_color(rgb(theme().bg_base))
                .child(SharedString::from("Conflict")),
        );
    }
    Some(file_row.into_any_element())
}

/// PERF: build one unstaged tree row (index `row_index` into `unstaged_tree`).
pub(crate) fn render_unstaged_tree_row(
    this: &KagiApp,
    row_index: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let row = panel.unstaged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-us-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            // Look up the original path to check if conflicted
            let path = panel.unstaged.get(fi).map(|f| f.path.clone());
            let is_conflicted_file = path
                .as_ref()
                .map(|p| panel.is_conflicted(p))
                .unwrap_or(false);
            let (badge, badge_color, _) = status_badge(&change, is_conflicted_file);
            let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
            let stat = path.as_ref().and_then(|p| panel.unstaged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| !*st && fp == p);

            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
                cx.notify();
            });
            let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_stage_file(fi);
                cx.notify();
            });
            let row_bg = if is_conflicted_file {
                theme().diff_removed_bg
            } else if is_sel {
                theme().selected
            } else {
                theme().panel
            };
            let mut file_row = div()
                .id(("cp-us-file", fi))
                .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .pl(theme::scaled_px(8.0 + indent))
                .pr(theme::scaled_px(2.0))
                .py_px()
                .bg(rgb(row_bg))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(file_click)
                .child(
                    div()
                        .w(theme::scaled_px(12.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(badge_color))
                        .child(SharedString::from(badge)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .truncate()
                        .child(name.clone()),
                )
                .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
            if !is_conflicted_file {
                // W17-DISCARD / ADR-0083: right-click opens the file context menu
                // (Discard lives there). Untracked rows are discardable too —
                // deleted from disk after an ODB backup.
                let menu_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _window, cx| {
                    this.file_menu = Some((fi, e.position));
                    cx.stop_propagation();
                    cx.notify();
                });
                file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
                file_row = file_row.child(
                    KagiButton::accent(("cp-us-stage-btn", fi), "Stage", theme().color_success, cx)
                        .xsmall()
                        .ml_2()
                        .flex_shrink_0()
                        .on_click(stage_click),
                );
            } else {
                file_row = file_row.child(
                    div()
                        .id(("cp-us-conflict-badge", fi))
                        .ml_2()
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .flex_shrink_0()
                        .bg(rgb(theme().color_blocker))
                        .text_xs()
                        .text_color(rgb(theme().bg_base))
                        .child(SharedString::from("Conflict")),
                );
            }
            Some(file_row.into_any_element())
        }
    }
}

/// PERF: build one staged row in flat view (index `fi` into `staged`).
pub(crate) fn render_staged_flat_row(
    this: &KagiApp,
    fi: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let f = panel.staged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let (badge, badge_color, _conflicted) = status_badge(&f.change, false);
    let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
    let stat = panel.staged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| *st && &f.path == p);

    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
        cx.notify();
    });
    let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.do_unstage_file(fi);
        cx.notify();
    });
    Some(
        div()
            .id(("cp-st-flat-file", fi))
            .when(wip_hit, |el| el.bg(rgb(theme().selected)))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_2()
            .py_px()
            .bg(rgb(if is_sel {
                theme().selected
            } else {
                theme().panel
            }))
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(file_click)
            .child(
                div()
                    .w(theme::scaled_px(12.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(badge_color))
                    .child(SharedString::from(badge)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .text_color(rgb(theme().text_main))
                    .overflow_hidden()
                    .truncate()
                    .child(SharedString::from(name)),
            )
            .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
            .child(
                KagiButton::accent(
                    ("cp-st-flat-unstage-btn", fi),
                    "Unstage",
                    theme().color_warning,
                    cx,
                )
                .xsmall()
                .ml_2()
                .flex_shrink_0()
                .on_click(unstage_click),
            )
            .into_any_element(),
    )
}

/// PERF: build one staged tree row (index `row_index` into `staged_tree`).
pub(crate) fn render_staged_tree_row(
    this: &KagiApp,
    row_index: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let row = panel.staged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-st-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            let (badge, badge_color, _conflicted) = status_badge(&change, false);
            let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
            let path = panel.staged.get(fi).map(|f| f.path.clone());
            let stat = path.as_ref().and_then(|p| panel.staged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| *st && fp == p);

            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
                cx.notify();
            });
            let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_unstage_file(fi);
                cx.notify();
            });
            Some(
                div()
                    .id(("cp-st-file", fi))
                    .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(theme::scaled_px(8.0 + indent))
                    .pr(theme::scaled_px(2.0))
                    .py_px()
                    .bg(rgb(if is_sel {
                        theme().selected
                    } else {
                        theme().panel
                    }))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(file_click)
                    .child(
                        div()
                            .w(theme::scaled_px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .overflow_hidden()
                            .truncate()
                            .child(name.clone()),
                    )
                    .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
                    .child(
                        KagiButton::accent(
                            ("cp-st-unstage-btn", fi),
                            "Unstage",
                            theme().color_warning,
                            cx,
                        )
                        .xsmall()
                        .ml_2()
                        .flex_shrink_0()
                        .on_click(unstage_click),
                    )
                    .into_any_element(),
            )
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Commit Panel renderer (T025)
// ──────────────────────────────────────────────────────────────

/// Render the Commit Panel: unstaged/staged sections + diff viewer + message input + commit button.
///
/// Layout (top to bottom in right panel):
/// 1. Unstaged (N)  [flat|tree] toggle
/// 2. Staged (M)
/// 3. Diff viewer (flex_1)
/// 4. Message input (T014 pattern — simple key handler)
/// 5. Warning (if unstaged remain)
/// 6. Commit button (disabled when staged=0 or message empty)
pub(crate) fn render_commit_panel(
    panel: CommitPanelState,
    panel_width: f32,
    commit_input: Option<Entity<InputState>>,
    template_mode: bool,
    template_inputs: Option<[Entity<InputState>; 6]>,
    // PERF: WIP highlight is now recomputed per visible row from `this.main_diff`
    // inside the uniform_list processors; this parameter is retained for the
    // stable call-site signature.
    _active_wip: Option<(bool, PathBuf)>,
    smart: smart_commit::SmartCommitState,
    preview: Option<kagi_git::CommitPreview>,
    unstaged_scroll_handle: UniformListScrollHandle,
    staged_scroll_handle: UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // theme().change_dir now sourced from theme().change_dir (W9-THEME).

    let tree_view = panel.tree_view;
    let unstaged_count = panel.unstaged.len();
    let staged_count = panel.staged.len();
    // W17-DISCARD: count discard-eligible unstaged files (exclude untracked,
    // which the panel surfaces as `Added` rows, and conflicted files).
    // ADR-0083: untracked (`Added`) rows ARE discardable (deleted with backup),
    // so they count toward enabling "Discard all" — only conflicted rows are
    // excluded. Must mirror `discard_partition`.
    let discard_eligible_count = panel
        .unstaged
        .iter()
        .filter(|f| !panel.is_conflicted(&f.path))
        .count();
    // T026 / T-COMMIT-009: can_commit uses the effective message — in template
    // mode the assembled fields, else the plain Input value (headless: commit_msg).
    let input_msg_nonempty = if template_mode {
        // Non-empty when summary or any field yields a non-empty assembled message.
        template_inputs
            .as_ref()
            .map(|inp| {
                let fields = kagi_git::TemplateFields::new(
                    inp[0].read(cx).value().to_string(),
                    inp[1].read(cx).value().to_string(),
                    inp[2].read(cx).value().to_string(),
                    inp[3].read(cx).value().to_string(),
                    inp[4].read(cx).value().to_string(),
                    inp[5].read(cx).value().to_string(),
                );
                !kagi_git::assemble(&fields).trim().is_empty()
            })
            .unwrap_or(false)
    } else {
        commit_input
            .as_ref()
            .map(|e| !e.read(cx).value().trim().is_empty())
            .unwrap_or(!panel.commit_msg.trim().is_empty())
    };
    let can_commit = !panel.staged.is_empty() && input_msg_nonempty;
    let has_unstaged_warning = !panel.unstaged.is_empty() && staged_count > 0;
    // PERF: selected_file is read per visible row from `this.commit_panel`
    // inside the uniform_list processors, not captured here.

    // ── View switch: segmented [List | Tree] (T-UI-002) ──────
    let list_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() {
            panel.tree_view = false;
        }
        cx.notify();
    });
    let tree_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() {
            panel.tree_view = true;
        }
        cx.notify();
    });
    let seg = |id: &'static str, label: &'static str, active: bool| {
        div()
            .id(id)
            .px_1p5()
            .py_px()
            .text_xs()
            .bg(rgb(if active {
                theme().selected
            } else {
                theme().surface
            }))
            .text_color(rgb(if active {
                theme().text_main
            } else {
                theme().text_muted
            }))
            .hover(|st| st.text_color(rgb(theme().text_main)).cursor_pointer())
            .child(SharedString::from(label))
    };
    let toggle_btn = div()
        .flex()
        .flex_row()
        .rounded_sm()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(theme().surface))
        .child(seg("cp-view-list", "List", !tree_view).on_click(list_click))
        .child(seg("cp-view-tree", "Tree", tree_view).on_click(tree_click));

    // ── Helper: build file rows for a section ────────────────
    // Returns a Vec of (element, depth, name, is_conflicted) as IntoElement.
    // We render inline to avoid capture issues.

    // ── Unstaged section ─────────────────────────────────────
    // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

    // Unstaged ヘッダ行 (固定 — flex_shrink_0 で高さを保持)
    let unstaged_header = div()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!("Unstaged ({})", unstaged_count))),
        )
        .when(unstaged_count > 0, |el| {
            let stage_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.do_stage_all();
                cx.notify();
            });
            el.child(
                div()
                    .id("cp-stage-all")
                    .mr_2()
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .text_color(rgb(theme().color_success))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(stage_all_click)
                    .child(SharedString::from("Stage all")),
            )
        })
        // W17-DISCARD: "Discard all" — disabled (muted, no handler) at 0 targets.
        .when(unstaged_count > 0, |el| {
            let discard_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.open_discard_all_modal();
                cx.notify();
            });
            let enabled = discard_eligible_count > 0;
            let mut btn = div()
                .id("cp-discard-all")
                .mr_2()
                .px_1p5()
                .py_px()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_xs()
                .child(SharedString::from("Discard all"));
            if enabled {
                btn = btn
                    .text_color(rgb(theme().color_blocker))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(discard_all_click);
            } else {
                btn = btn.text_color(rgb(theme().text_muted));
            }
            el.child(btn)
        })
        .child(toggle_btn);

    // PERF: unstaged file rows are virtualized via `uniform_list` (built from
    // free row functions reading `this.commit_panel`), not a prebuilt div.
    let unstaged_row_count = if tree_view {
        panel.unstaged_tree.len()
    } else {
        unstaged_count
    };

    // ── Staged section ───────────────────────────────────────
    // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

    // Staged ヘッダ行 (固定)
    let staged_header = div()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!("Staged ({})", staged_count))),
        )
        .when(staged_count > 0, |el| {
            let unstage_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.do_unstage_all();
                cx.notify();
            });
            el.child(
                div()
                    .id("cp-unstage-all")
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .text_color(rgb(theme().color_warning))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(unstage_all_click)
                    .child(SharedString::from("Unstage all")),
            )
        });

    // PERF: staged file rows are virtualized via `uniform_list` (built from
    // free row functions reading `this.commit_panel`), not a prebuilt div.
    let staged_row_count = if tree_view {
        panel.staged_tree.len()
    } else {
        staged_count
    };

    // ── plain ⇄ template mode toggle (T-COMMIT-009) ───────────────
    let mode_toggle = {
        let toggle_click = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
            this.toggle_commit_template_mode(window, cx);
        });
        let label = if template_mode {
            "Plain message"
        } else {
            "Template fields"
        };
        div()
            .id("cp-template-toggle")
            .px_1p5()
            .py_px()
            .rounded_sm()
            .text_xs()
            .bg(rgb(theme().surface))
            .text_color(rgb(theme().color_branch))
            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
            .on_click(toggle_click)
            .child(SharedString::from(format!("⇄ {}", label)))
    };

    // ── Commit message input (T026/T-COMMIT-009) ──────────────────
    // Template mode renders the six structured fields (gpui-component Input for
    // each — no hand-written widgets); plain mode renders the single Input.
    let msg_input_wrapper: gpui::AnyElement = if template_mode {
        if let Some(inp) = template_inputs.clone() {
            let [ty, scope, summary, body, test, risk] = inp;

            // Labeled single-line field.
            let field = |label: &'static str, state: &Entity<InputState>| {
                div()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_label))
                            .child(SharedString::from(label)),
                    )
                    .child(Input::new(state).appearance(true).bordered(true))
            };

            // type quick-pick chips (also free-typeable in the type field above).
            let mut chips = div().flex().flex_row().flex_wrap().gap_1();
            for &choice in kagi_git::TYPE_CHOICES {
                let ty_state = ty.clone();
                let pick = cx.listener(move |_this, _e: &gpui::ClickEvent, window, cx| {
                    ty_state.update(cx, |s, cx| s.set_value(choice.to_string(), window, cx));
                });
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("cp-type-chip-{}", choice)))
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .text_xs()
                        .bg(rgb(theme().surface))
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(pick)
                        .child(SharedString::from(choice)),
                );
            }

            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(field("type", &ty))
                .child(chips)
                .child(field("scope", &scope))
                .child(field("summary", &summary))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_label))
                                .child(SharedString::from("body")),
                        )
                        .child(Input::new(&body).appearance(true).bordered(true)),
                )
                .child(field("test", &test))
                .child(field("risk", &risk))
                .into_any_element()
        } else {
            // Template mode requested but inputs not yet created (no &mut Window
            // here) — should not occur because the toggle creates them.
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(template fields unavailable)"))
                .into_any_element()
        }
    } else if let Some(ref input_entity) = commit_input {
        // Use gpui-component Input element — handles IME, clipboard, arrow keys, etc.
        Input::new(input_entity)
            .appearance(true)
            .bordered(true)
            .into_any_element()
    } else {
        // Fallback for headless / no-window case (should not occur in normal UI flow).
        div()
            .px_2()
            .py_1()
            .bg(rgb(theme().bg_base))
            .rounded_sm()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("(commit message input unavailable)"))
            .into_any_element()
    };

    // ── Commit button ─────────────────────────────────────────
    let commit_btn = if can_commit {
        let commit_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.open_commit_plan_modal(cx);
            cx.notify();
        });
        Button::new("cp-commit-btn")
            .label(SharedString::from(format!(
                "Commit ({} file{})",
                staged_count,
                if staged_count == 1 { "" } else { "s" }
            )))
            .primary()
            .small()
            .mt_1()
            .w_full()
            .on_click(commit_click)
            .into_any_element()
    } else {
        // Tell the user exactly why the button is disabled.
        let reason = if staged_count == 0 && !input_msg_nonempty {
            "Commit — stage a file and enter a message first"
        } else if staged_count == 0 {
            "Commit — stage at least one file first"
        } else {
            "Commit — enter a commit message first"
        };
        div()
            .id("cp-commit-btn-disabled")
            .mt_1()
            .w_full()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(reason))
            .into_any_element()
    };

    // ── Smart Commit Message toolbar (T-COMMIT-016) ───────────
    // Rule-based "Suggest" is always available; "Generate with Local LLM" is
    // offered only when an Ollama server is detected and the user opted in.
    let staged_empty = panel.staged.is_empty();
    let smart_toolbar = {
        // Small reusable button factory.
        let pill = |id: &'static str, label: SharedString, enabled: bool, accent: u32| {
            let mut b = div()
                .id(id)
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .bg(rgb(theme().surface))
                .text_color(rgb(if enabled { accent } else { theme().text_muted }))
                .child(label);
            if enabled {
                b = b.hover(|s| s.bg(rgb(theme().selected)).cursor_pointer());
            }
            b
        };

        // Suggest — one button: uses the local LLM when it's usable (green),
        // otherwise the rule-based draft (blue). Shows "Generating…" while the
        // LLM runs. (The separate "Generate with Local LLM" button is gone.)
        let llm_on = smart.llm_offered();
        let suggest_enabled = !staged_empty && !smart.generating;
        let suggest_color = if llm_on {
            theme().color_success
        } else {
            theme().color_branch
        };
        let suggest_btn: gpui::AnyElement = if smart.generating {
            // Animated braille "dots" spinner while the LLM generates (user
            // request — the spinning-dots glyph). The whole panel re-renders each
            // animation frame, so the closure rebuilds a fresh single-child div.
            use gpui::AnimationExt as _;
            const FRAMES: [&str; 10] = [
                "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}",
                "\u{2827}", "\u{2807}", "\u{280F}",
            ];
            let spinner = div()
                .text_xs()
                .text_color(rgb(suggest_color))
                .with_animation(
                    "cp-smart-spinner",
                    gpui::Animation::new(Duration::from_millis(800)).repeat(),
                    |el, delta| {
                        let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
                        el.child(SharedString::from(FRAMES[i]))
                    },
                );
            div()
                .id("cp-smart-suggest")
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .bg(rgb(theme().surface))
                .text_color(rgb(suggest_color))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(spinner)
                .child(SharedString::from("Generating…"))
                .into_any_element()
        } else {
            let mut b = pill(
                "cp-smart-suggest",
                SharedString::from("Suggest"),
                suggest_enabled,
                suggest_color,
            );
            if suggest_enabled {
                let suggest_click = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    if llm_on {
                        this.smart_generate(window, cx);
                    } else {
                        this.smart_suggest(window, cx);
                    }
                });
                b = b.on_click(suggest_click);
            }
            b.into_any_element()
        };

        // Lang toggle (En / 日本語).
        let lang_label = match smart.lang {
            message_gen::Lang::En => "Lang: EN",
            message_gen::Lang::Ja => "Lang: 日本語",
        };
        let lang_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
            this.smart_commit.toggle_lang();
            cx.notify();
        });
        let lang_btn = pill(
            "cp-smart-lang",
            SharedString::from(lang_label),
            true,
            theme().text_main,
        )
        .on_click(lang_click);

        // ADR-0090: the Style (CC vs Plain) toggle was removed — style now
        // follows the commit-panel mode (template → Conventional, plain → Plain).

        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(suggest_btn)
            .child(lang_btn);

        // "Generate with Local LLM" is folded into Suggest (above). When the LLM
        // is detected but not yet enabled, offer an opt-in affordance so the user
        // can turn it on (after which Suggest goes green and uses it).
        if smart.ollama_available && !smart.llm_enabled {
            let enable_click = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
                this.smart_generate(window, cx);
            });
            let enable_btn = pill(
                "cp-smart-enable-llm",
                SharedString::from("Enable Local LLM…"),
                !staged_empty,
                theme().color_success,
            )
            .when(!staged_empty, |el| el.on_click(enable_click));
            row = row.child(enable_btn);
        }

        // "Local LLM available" indicator.
        let mut col = div().flex().flex_col().gap_px().child(row);
        if smart.ollama_available {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().color_success))
                    .child(SharedString::from("● Local LLM available")),
            );
        }
        // Transient status line (rule-based inserted / generating / fell back).
        if let Some(ref status) = smart.status {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(status.clone())),
            );
        }
        col
    };

    // ── Commit preview header (T-COMMIT-001) ──────────────────
    // Shows what the *next* commit contains: staged count, A/M/D summary,
    // target branch (detached/unborn handled), and author.  Pure read from
    // `commit_preview()`; hidden if the preview could not be built.
    let preview_block: gpui::AnyElement = if let Some(ref pv) = preview {
        let count_line = format!(
            "{} file{} staged",
            pv.staged_count,
            if pv.staged_count == 1 { "" } else { "s" }
        );
        let summary = pv.summary();
        let mut col = div()
            .flex()
            .flex_col()
            .gap_px()
            // Line 1: count + A/M/D summary
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .child(SharedString::from(count_line)),
                    )
                    .when(!summary.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(summary)),
                        )
                    }),
            );
        // Line 2: target branch
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(format!("→ {}", pv.target_branch))),
        );
        // Line 3: author
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(format!("by {}", pv.author))),
        );
        col.into_any_element()
    } else {
        div().into_any_element()
    };

    // ── Assemble panel ───────────────────────────────────────
    // T-UI-003: diff ボックス廃止。Unstaged/Staged 箱が flex_1 で全体を占める(1:1)。
    div()
        // `panel_width` is the unscaled, persisted right-panel width; scale at
        // render so it tracks zoom (the Panel divider drag uses the same space).
        .w(theme::scaled_px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        // Header
        .child(
            div()
                .flex_shrink_0()
                .px_2()
                .py_1()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from("Commit Panel")),
        )
        // T-UI-003: ファイル領域コンテナ (flex_1 + min_h(0)) — diff 廃止でフル高さ
        .child(
            div()
                .id("cp-files-container")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                // Unstaged ヘッダ (固定)
                .child(unstaged_header)
                // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                .child(
                    div()
                        .id("cp-unstaged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .flex()
                        .flex_col()
                        .child({
                            let handle = unstaged_scroll_handle.clone();
                            with_vertical_scrollbar(
                                "cp-unstaged-list-scroll",
                                &handle,
                                uniform_list(
                                    "cp-unstaged-list",
                                    unstaged_row_count,
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _window, cx| {
                                            let tree = this
                                                .commit_panel
                                                .as_ref()
                                                .map(|p| p.tree_view)
                                                .unwrap_or(false);
                                            range
                                                .filter_map(|i| {
                                                    if tree {
                                                        render_unstaged_tree_row(this, i, cx)
                                                    } else {
                                                        render_unstaged_flat_row(this, i, cx)
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(unstaged_scroll_handle)
                                .flex_1()
                                .min_h(px(0.)),
                                false,
                            )
                        }),
                )
                // Staged ヘッダ (固定)
                .child(staged_header)
                // Staged スクロールボックス — PERF: virtualized uniform_list.
                .child(
                    div()
                        .id("cp-staged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .flex()
                        .flex_col()
                        .child({
                            let handle = staged_scroll_handle.clone();
                            with_vertical_scrollbar(
                                "cp-staged-list-scroll",
                                &handle,
                                uniform_list(
                                    "cp-staged-list",
                                    staged_row_count,
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _window, cx| {
                                            let tree = this
                                                .commit_panel
                                                .as_ref()
                                                .map(|p| p.tree_view)
                                                .unwrap_or(false);
                                            range
                                                .filter_map(|i| {
                                                    if tree {
                                                        render_staged_tree_row(this, i, cx)
                                                    } else {
                                                        render_staged_flat_row(this, i, cx)
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(staged_scroll_handle)
                                .flex_1()
                                .min_h(px(0.)),
                                false,
                            )
                        }),
                ),
        )
        // Commit footer: message input + warning + button
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .px_2()
                .py_1()
                .gap_1()
                .bg(rgb(theme().surface))
                // T-COMMIT-001: staged preview (count / A·M·D / branch / author)
                .child(preview_block)
                // Message label + plain⇄template toggle
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_xs().text_color(rgb(theme().text_label)).child(
                            SharedString::from(if template_mode {
                                "Commit message (template)"
                            } else {
                                "Commit message"
                            }),
                        ))
                        .child(mode_toggle),
                )
                // Template mode stacks six fields and overflows the footer; bound
                // its height and let it scroll so the commit button stays reachable.
                .child(if template_mode {
                    div()
                        .id("cp-template-scroll")
                        .max_h(theme::scaled_px(300.))
                        .overflow_y_scroll()
                        .child(msg_input_wrapper)
                        .into_any_element()
                } else {
                    msg_input_wrapper
                })
                // Smart Commit Message toolbar (Suggest / Generate / toggles)
                .child(smart_toolbar)
                // Unstaged warning
                .when(has_unstaged_warning, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(i18n::unstaged_not_included(
                                unstaged_count,
                            ))),
                    )
                })
                // Commit button
                .child(commit_btn)
                // T-COMMIT-011: Amend the previous commit (unpushed only —
                // the plan blocks pushed/merge/etc.). Mode follows what the
                // user has provided: staged changes, a new message, or both.
                .child({
                    let amend_click = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
                        let staged = this
                            .commit_panel
                            .as_ref()
                            .map(|p| !p.staged.is_empty())
                            .unwrap_or(false);
                        let msg = this
                            .commit_input
                            .as_ref()
                            .map(|i| !i.read(cx).value().trim().is_empty())
                            .unwrap_or(false);
                        let mode = match (msg, staged) {
                            (true, true) => AmendMode::Both,
                            (false, true) => AmendMode::Staged,
                            (true, false) => AmendMode::MessageOnly,
                            (false, false) => {
                                this.status_footer = FooterStatus::Idle(SharedString::from(
                                    Msg::AmendNeedMessageOrStaged.t(),
                                ));
                                cx.notify();
                                return;
                            }
                        };
                        this.open_amend_modal(mode, cx);
                        cx.notify();
                    });
                    div()
                        .id("cp-amend-btn")
                        .mt_1()
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(rgb(theme().surface))
                        .text_sm()
                        .text_color(rgb(theme().color_warning))
                        .on_click(amend_click)
                        .hover(|st| st.bg(rgb(theme().selected)))
                        .child(SharedString::from("Amend last commit…"))
                }),
        )
}
