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
use kagi_ui_core::{
    tooltip::HoverTooltip as _,
    variable_list::{variable_list, VariableListState},
};
use std::rc::Rc;

// T-SPLIT-HELPERS-001 / ADR-0116 Wave 3: the commit-panel, badge, and file-menu
// renderers moved to focused sibling modules. Re-export them here so the existing
// `use super::render_helpers::*;` call sites keep resolving without touching the
// render_*.rs callers (public paths preserved).
// ADR-0117: the file-history renderers are now internal to `file_history_render`
// (the `Entity<FileHistoryView>` renders itself), so they are no longer
// re-exported here.
pub(crate) use super::badges::*;
pub(crate) use super::file_menu::*;
pub(crate) use diff_header::DiffHeader;
pub(crate) use kagi_ui_core::header_fit::{header_button, HeaderFit};

mod diff_header;
mod empty_state;
pub(crate) use empty_state::empty_state;
pub(crate) mod diff_state;
mod metadata;
pub(crate) use metadata::subject_tooltip;
pub(crate) mod row_overlay;

/// issue #414: chokepoint that neutralizes terminal control bytes in remote-origin
/// display text (PR titles/handles/refs/check names, branch & stash labels) before
/// it reaches a render site. Bidi/zero-width are left intact (may be legitimate).
pub(crate) fn safe_text(s: &str) -> gpui::SharedString {
    gpui::SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(s))
}

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

/// Nagi Dark paints a solid blue selection, unlike Apple's translucent graph
/// wash. Share file-row contrast ink during both incoming and outgoing fades.
pub(super) fn graph_selection_ink(
    selection: f32,
    window_active: bool,
    background: gpui::Hsla,
) -> Option<(gpui::Hsla, gpui::Hsla)> {
    if theme().slug.as_ref() == "nagi-dark" {
        super::diffstat_bar::file_selection_ink(selection, window_active, background)
    } else {
        None
    }
}

pub(crate) fn render_rows(
    rows: &[CommitRow],
    avatar_images: &HashMap<String, std::sync::Arc<gpui::Image>>,
    range: std::ops::Range<usize>,
    window_active: bool,
    badge_col_w: f32,
    graph_col_w: f32,
    subject_min: f32,
    graph_compact: bool,
    graph_scroll_x: f32,
    stash_lanes: &[usize],
    solo_visible: Option<&HashSet<CommitId>>,
    // #354: (rows above the commits in the same list, total list size).
    a11y_offset_size: (usize, usize),
    // #985: where the selected row is drawn, for the context-menu key.
    anchor: &super::menu_keys::RowAnchor,
    motion: &super::render_body::motion::GraphMotion,
    cx: &mut Context<KagiApp>,
) -> Vec<impl IntoElement> {
    let rh = row_height(graph_compact);
    // One reference instant for visible ages and their accessibility descriptions.
    let now = kagi_ui_core::time::now_unix_secs();

    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            // T-PERF-RENDER-002 (ADR-0116 Wave 2): `row` stays a `&CommitRow`
            // borrowed from `rows`; the click/context handlers capture `ix`
            // (not the row), and every field read either copies a `Copy` value
            // or bumps an Arc (`SharedString`) / clones a small Vec, so the
            // whole-row clone per visible row is unnecessary.
            let key = motion.key(a11y_offset_size.0 + ix);
            let is_selected = key.is_some_and(|key| motion.selected(key));
            let selection = key.map_or(0., |key| motion.selection(key));
            let is_dimmed = solo_visible.is_some_and(|visible| !visible.contains(&row.id));

            // Graph keeps its normal stripe beneath the accent wash (#1485).
            let default_bg: gpui::Hsla = rgb(if ix % 2 == 0 {
                theme().bg_base
            } else {
                theme().bg_row_alt
            })
            .into();
            let wash = theme().selection_wash(window_active);
            let row_bg = default_bg.blend(wash.alpha(wash.a * selection));
            let ink = graph_selection_ink(selection, window_active, row_bg);
            let foreground =
                ink.map_or_else(|| rgb(theme().text_main).into(), |(primary, _)| primary);
            let secondary_foreground =
                ink.map_or_else(|| rgb(theme().text_sub).into(), |(_, secondary)| secondary);
            let muted_foreground = ink.map_or_else(
                || rgb(theme().text_muted).into(),
                |(_, secondary)| secondary,
            );
            // Swimlane lane band (Gitru-style): an inner horizontal strip drawn
            // BEHIND the graph column only and tinted by this row's lane colour.
            // It is a separate absolute layer — NOT the row background — with
            // vertical inset so adjacent bands don't touch and the row stays a
            // neutral list. The message column is intentionally left neutral so
            // text stays readable. Selected active rows retain the lane band
            // under their wash; inactive rows keep the existing grey fill.
            // Tuple = (normal, hover) wash colours.
            let lane_band: Option<(gpui::Hsla, gpui::Hsla)> =
                if theme::graph_lane_compact() && (selection < 1. || window_active) {
                    let c = theme().lane_color(row.node_color);
                    let na = theme().lane_band_alpha(false);
                    let ha = theme().lane_band_alpha(true);
                    let normal = gpui::hsla(c.h, c.s, c.l, na);
                    let hover = gpui::hsla(c.h, c.s, c.l, ha);
                    if selection > 0. {
                        // Both incoming and outgoing lane bands retain their tint.
                        let selected = default_bg
                            .blend(normal)
                            .blend(wash.alpha(wash.a * selection));
                        Some((selected, selected))
                    } else {
                        Some((normal, hover))
                    }
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
            let visible_lanes = graph_view::lanes_for_width(theme::scaled(graph_col_w));

            // Pointer selection hands keyboard ownership to the Graph, never
            // during rendering or while the user is typing in another field.
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
                if let Some(root) = &this.root_focus {
                    root.focus(window, cx);
                }
                this.commit_menu = None;
                this.select(ix);
                cx.stop_propagation();
                // ADR-0089 Phase 2c: kick off the remote changed-files load now
                // that we have `cx` (the render trigger is a fallback for
                // keyboard navigation). Idempotent.
                if this.remote_view.is_some() && this.ui().selected == Some(ix) {
                    this.load_remote_changed_files(ix, cx);
                }
                cx.notify();
            });
            let context_click_handler =
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    // MenuKeys captures Graph as its Escape return target.
                    if let Some(root) = &this.root_focus {
                        root.focus(window, cx);
                    }
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

            super::list_a11y::list_option(
                "commit-list",
                div().id(ix),
                a11y_offset_size.0 + ix,
                a11y_offset_size.1,
                super::list_a11y::commit_label(row, now),
                is_selected,
            )
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
            .when(selection > 0., |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(rgb(if window_active {
                            theme().ui_accent()
                        } else {
                            theme().text_muted
                        }))
                        .opacity(selection)
                        .when(cfg!(feature = "gui-e2e"), |bar| {
                            bar.child(super::e2e::measure_inside("graph-selection-bar"))
                        }),
                )
                .when(is_selected, |el| el.child(anchor.probe()))
            })
            .h(px(rh))
            .when(ix == 0 && cfg!(feature = "gui-e2e"), |el| {
                el.child(super::e2e::measure_inside("graph-commit-row"))
            })
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(super::e2e::measure_inside(format!("graph-row-{ix}")))
            })
            .bg(row_bg)
            .text_color(foreground)
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
                is_selected,
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
                                .hover(|s| s.bg(band_hover))
                                .when(cfg!(feature = "gui-e2e"), |el| {
                                    el.child(super::e2e::measure_inside(format!(
                                        "graph-lane-band-{ix}"
                                    )))
                                }),
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
                            this.scroll_graph_by(&e.delta, graph_col_w, cx);
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
                                    graph_view::GraphNode::Commit {
                                        is_head: row.is_head,
                                        is_merge: row.is_merge,
                                    },
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
                        let ring_d = graph_view::avatar_node_diameter();
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
                            .child(div().text_color(gpui::white()).text_xs().child(avatar_init)),
                    }
                })
            })
            .child(metadata::subject(row, ix, subject_min, foreground))
            // Issue #337: AI-agent provenance badge (nothing when the
            // commit is unclassifiable — prefer no badge to a false one).
            .when_some(row.provenance.as_ref(), |el, prov| {
                el.child(badges::render_provenance_badge(prov, ix))
            })
            .child(metadata::author(row, ix, secondary_foreground))
            .child(metadata::age(row, ix, now, muted_foreground))
            // Row-wide help preserves complete subjects; child ref/author
            // tooltips retain priority.
            .hover_tooltip_builder(("graph-row-hover-help", ix), {
                let subject = row.summary.clone();
                move |window, cx| metadata::subject_tooltip(subject.clone(), window, cx)
            })
        })
        .collect()
}

/// Pagination Button beside the commit ListBox, never a virtual Option.
/// Shown at the current limit; click grows the walk by [`COMMIT_PAGE_STEP`].
pub(crate) fn render_load_more_row(
    graph_compact: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let rh = row_height(graph_compact);
    let label = crate::ui::i18n::Msg::LoadMoreCommits.t();
    super::e2e::measure_control(
        "commit-load-more",
        super::list_a11y::list_action(
            "commit-load-more",
            div()
                .id("commit-load-more")
                .h(px(rh))
                .flex_shrink_0()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(theme().accent_text_on(theme().bg_base)))
                .hover(|s| {
                    s.bg(rgb(theme().selected))
                        .text_color(rgb(theme().accent_text_on(theme().selected)))
                })
                .on_click(cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
                    this.load_more_commits(cx);
                }))
                .child(SharedString::from(label)),
            label,
        ),
    )
}

// Note: render_detail_panel was extracted to src/ui/inspector.rs (W2-INSPECTOR).

// Note: render_loading_placeholder lives in render_body.rs (its only caller;
// moved there under the ADR-0165 change to keep this file under 800 LOC).

/// Construct an owner-retained diff list (item count 0 — the caller syncs
/// source/layout identity and count in `render_diff_list`).
/// Each embedding retains its own geometry and invalidation memo, so no
/// measurements or scroll positions cross owner boundaries. The overdraw tuning
/// lives in one place. `px(1000.)` matches gpui-component's own `TextView` (a similarly
/// line-oriented variable-height list).
pub(crate) fn new_diff_list_state() -> VariableListState {
    VariableListState::new(0, px(1000.))
}

/// ADR-0117 / T-DIFF-WRAP-001: the diff "list body" (header line + virtualized
/// rows + scrollbar), parameterized over the entity context `V` so it can be
/// rendered from `MainDiffPane` (the standalone main diff), `FileHistoryView` (its
/// embedded diff pane), or `EditorWorkspaceView` (the hunks pane). `header`
/// carries the optional standalone header buttons (Back / History) — `None`
/// when embedded, which supplies its own Back. Never read an entity back via
/// `cx` here — this runs during render (the rendering entity is already
/// borrowed → panic).
///
/// Renders through Kagi's variable-height list so a soft-wrapped diff line
/// grows its row instead of being clipped to a fixed row height.
/// Construct one retained `VariableListState` per owner via [`new_diff_list_state`].
///
/// Source/layout changes invalidate measurements, including a different
/// same-sized source. Highlight-only changes preserve the height-source identity.
/// Offscreen heights are estimates; visible wrapped rows and expansions replace
/// them with their actual sizes. Unchanged renders do not rebuild the height tree.
///
/// #495: every embedding's highlight is requested here, off the UI thread,
/// once per (rows, theme) — see `diff_view::highlight`.
///
/// #842: `hunk` puts Stage / Unstage hunk on the hunk headers (the Commit
/// Panel's diff); every other embedding passes `None`.
pub(crate) fn render_diff_list<V: super::diff_view::highlight::DiffHighlightHost>(
    view: MainDiffView,
    mut header: DiffHeader,
    scroll_handle: VariableListState,
    list_layout: &kagi_ui_core::diff_list::DiffListLayout,
    hunk: Option<super::diff_view::hunk_action::HunkAction>,
    owner: super::diff_selection::SelectionOwner,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let presentation_only = header.presentation_only;
    super::diff_view::highlight::ensure_highlight(&view, cx);
    // ADR-0124: virtualize over shared paired indices. Unchanged row ownership
    // reuses both pairing and move detection across frames and embeddings.
    let split = super::theme::diff_split();
    let projection = split.then(|| super::diff_split::split_projection(&view.rows));
    let sel_key = super::diff_selection::SelectionKey::new(
        owner,
        list_layout.selection_owner(),
        view.height_source,
    );
    super::diff_selection::sync(sel_key);
    // Focus belongs to the retained list, not its replaceable content.
    let focus_key = list_layout.selection_owner().token();
    // #351: an embedding's overlay adds one list item under each expanded row.
    let overlay = header.overlay.take();
    let previous_overlay = header.previous_overlay.take();
    let overlay_projection = overlay.as_ref().map(|overlay| overlay.projection());
    let items = overlay_projection
        .as_ref()
        .map(|projection| projection.items.clone());
    let row_count = match &items {
        Some(items) => items.len(),
        None => projection
            .as_ref()
            .map(|projection| projection.rows.len())
            .unwrap_or_else(|| view.rows.len()),
    };
    let retiring_split = std::cell::OnceCell::new();
    list_layout.sync(
        &scroll_handle,
        view.height_source,
        split,
        overlay.is_some(),
        row_count,
        |previous_split, previous_gutter, item| {
            // Only invoked on a same-source mode change. Reuse the already
            // cached pairing even when the destination is Unified.
            let split_rows = || {
                &projection
                    .as_ref()
                    .unwrap_or_else(|| {
                        retiring_split
                            .get_or_init(|| super::diff_split::split_projection(&view.rows))
                    })
                    .rows
            };
            let (source, expansion) = if previous_gutter {
                previous_overlay.as_ref()?.source_anchor(item)?
            } else if previous_split {
                let source = match split_rows().get(item)? {
                    super::diff_split::SplitDiffRow::Full(row) => Some(*row),
                    super::diff_split::SplitDiffRow::Pair { left, right } => left.or(*right),
                }?;
                (source, false)
            } else {
                (item, false)
            };
            if let Some(overlay) = &overlay_projection {
                overlay.item_for_source(source, expansion)
            } else if split {
                super::diff_split::split_index_of(split_rows(), source)
            } else {
                Some(source)
            }
        },
    );
    let title = view.title.clone();
    let stats = view.stats.clone();
    let media_body = view.side_pair.clone().map(|pair| {
        let pdf = pair.headers[0].id == "main-diff-pdf-old";
        let image = view.images.is_some();
        let state = match (pair.left.is_present(), pair.right.is_present()) {
            (true, true) => Msg::DiffImageModified,
            (true, false) => Msg::DiffImageDeleted,
            (false, true) => Msg::DiffImageAdded,
            _ => Msg::DiffBinaryNoText,
        };
        div()
            .id(if pdf {
                "main-diff-pdf-panel"
            } else {
                "main-diff-image-panel"
            })
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .flex()
            .flex_col()
            .when(image, |el| el.child(diff_state::caption(state)))
            .child(diff_state::side_pair(
                pair,
                [None, None],
                header.pdfs.as_ref(),
                if pdf {
                    ["main-diff-pdf-old-body", "main-diff-pdf-new-body"]
                } else {
                    ["main-diff-image-old-body", "main-diff-image-new-body"]
                },
                true,
            ))
            .into_any_element()
    });
    // Already an `Arc` on the view — this is a refcount bump, not a copy.
    let rows = view.rows;
    let rows_for_list = rows.clone();

    let header = header.render(title, stats, cx);

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
        .child(header)
        // ── Diff body: full remaining space ──────────────────────────────
        .child(if let Some(body) = media_body {
            body
        } else {
            // The owned geometry handle implements the public ScrollbarHandle
            // contract used by the same scrollbar as the other list panes.
            let scrollbar_handle = scroll_handle.clone();
            let selection_scroll = scroll_handle.clone();
            let selection_projection = projection.clone();
            let selection_items = items.clone();
            // User report: diff text rendered in the UI font (the root div's
            // `UI_FONT`), which is proportional — wrong for code, and it also
            // broke the `format!("{:5}", n)` line-number gutters, which only
            // line up in a monospace face. Set once here on the list BODY
            // (GPUI text styles cascade, so every row and both render modes
            // inherit it) rather than per-row; the header above keeps the UI
            // font, the same UI-chrome-over-mono-code split the conflict
            // editor uses.
            //
            // `MONO_FONT`, not `terminal::pick_font_family()`: the latter
            // prefers a Nerd Font for the terminal's icon glyphs, which a diff
            // has no use for, and the diff sits directly beside code rendered
            // by `InputState::code_editor` (the Editor Workspace's Diff ⇄
            // Snapshot tabs swap in the same pane), which draws with
            // gpui-component's `mono_font_family` — i.e. `MONO_FONT`. Matching
            // the neighbour matters more than the Nerd Font preference here.
            div()
                .id(("diff-selection-body", focus_key))
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .font_family(super::MONO_FONT)
                .child(with_vertical_scrollbar(
                    "main-diff-list-scroll",
                    &scrollbar_handle,
                    variable_list(scroll_handle, move |ix, window, cx| {
                        let base = match (&overlay_projection, &overlay) {
                            (Some(projection), Some(overlay)) => match projection.items.get(ix) {
                                Some(row_overlay::Item::Row(b)) => *b,
                                Some(row_overlay::Item::Expansion(b)) => {
                                    let (rows, len) = projection.expansion_rows(*b);
                                    return overlay.expansion(&rows[..len], window, cx);
                                }
                                None => return div().into_any_element(),
                            },
                            _ => ix,
                        };
                        // ADR-0124: split mode renders paired cells; unified keeps
                        // the original single-column rows.
                        match &projection {
                            Some(projection) => super::diff_split::render_main_diff_split_row(
                                &rows_for_list,
                                &projection.rows,
                                base,
                                sel_key,
                                &projection.moved,
                                overlay
                                    .as_ref()
                                    .map(|o| split_gutters(o, &projection.rows, base, cx)),
                                hunk.as_ref(),
                            ),
                            None => {
                                let row = render_main_diff_row(
                                    &rows_for_list,
                                    base,
                                    sel_key,
                                    hunk.as_ref(),
                                );
                                match &overlay {
                                    Some(o) => row_overlay::with_gutter(
                                        row_overlay::gutter(o.marker(base, None, cx)),
                                        row,
                                    ),
                                    None => row,
                                }
                            }
                        }
                    })
                    .flex_1()
                    .min_h(px(0.)),
                    true,
                ))
                .map(|element| {
                    if presentation_only {
                        return element.into_any_element();
                    }
                    super::diff_selection::keyboard::Pane {
                        element,
                        key: sel_key,
                        focus_key,
                        rows,
                        scroll: selection_scroll,
                        projection: selection_projection.clone(),
                        items: selection_items.clone(),
                        item_index: move |row| {
                            let base = selection_projection
                                .as_ref()
                                .and_then(|p| super::diff_split::split_index_of(&p.rows, row))
                                .unwrap_or(row);
                            selection_items
                                .as_ref()
                                .and_then(|items| {
                                    items
                                        .iter()
                                        .position(|item| *item == row_overlay::Item::Row(base))
                                })
                                .unwrap_or(base)
                        },
                    }
                    .into_any_element()
                })
                .into_any_element()
        })
}

/// The split view's two gutter cells for list row `base` (#351): a pair's
/// left/right markers on their own side; empty cells elsewhere.
fn split_gutters(
    overlay: &Rc<dyn row_overlay::RowOverlay>,
    srows: &[super::diff_split::SplitDiffRow],
    base: usize,
    cx: &mut gpui::App,
) -> (gpui::AnyElement, gpui::AnyElement) {
    use row_overlay::{gutter, GutterSide};
    match srows.get(base) {
        Some(super::diff_split::SplitDiffRow::Pair { left, right }) => (
            gutter(left.and_then(|row| overlay.marker(row, Some(GutterSide::Left), cx))),
            gutter(right.and_then(|row| overlay.marker(row, Some(GutterSide::Right), cx))),
        ),
        _ => (gutter(None), gutter(None)),
    }
}
// ADR-0121 B2: `render_main_diff_view` (the standalone KagiApp diff wrapper)
// moved into `main_diff_pane.rs` as `MainDiffPane`'s `Render` impl. The
// embedded users (File History / Editor Workspace) keep calling
// [`render_diff_list`] directly, as before.

/// W12-GCADOPT (§2.10): wrap a virtualized list in a relative flex column and
/// overlay a `gpui_component::scroll::Scrollbar` driven by the list's existing
/// scroll handle. The Scrollbar paints itself absolutely-positioned over the
/// container (relative(1.) size), so this is layout-non-destructive — the
/// inner list keeps its own `flex_1().min_h(0)` sizing. Colours follow the
/// gpui-component scrollbar theme fields, which `sync_gpui_component_theme`
/// keeps in step with kagi's palette.
/// `show_bar` controls whether the overlay scrollbar is rendered. `false` hides
/// it entirely (the list still scrolls via wheel/trackpad) — used for the commit
/// stage/unstage lists, which the user wants free of a visible scrollbar. When
/// `true` the bar follows the theme default (`cx.theme().scrollbar_show`, which
/// honours the macOS "show scroll bars" setting).
///
/// Generic over the public `ScrollbarHandle` contract so native lists and
/// Kagi-owned variable-height diff lists use the same scrollbar chrome.
pub(crate) fn with_vertical_scrollbar<H: gpui_component::scroll::ScrollbarHandle + Clone>(
    id: &'static str,
    handle: &H,
    list: impl IntoElement,
    show_bar: bool,
) -> gpui::Stateful<gpui::Div> {
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

/// macOS policy used by the PR table: auto-hide overlays content; persistent
/// bars have a dedicated lane, including the kit's Hover system mapping.
pub(crate) fn with_system_vertical_scrollbar<H: gpui_component::scroll::ScrollbarHandle + Clone>(
    id: &'static str,
    handle: &H,
    content: impl IntoElement,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let persistent = gpui_component::Theme::global(cx).scrollbar_show
        != gpui_component::scroll::ScrollbarShow::Scrolling;
    div()
        .id(id)
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .when(persistent, |area| area.pr(px(16.)))
        .when(cfg!(feature = "gui-e2e"), |area| {
            area.child(super::e2e::measure_inside(id))
        })
        .child(content)
        .child(Scrollbar::vertical(handle).id(id).when(persistent, |bar| {
            bar.scrollbar_show(gpui_component::scroll::ScrollbarShow::Always)
        }))
}
