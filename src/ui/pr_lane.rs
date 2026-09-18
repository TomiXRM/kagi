//! The PR's lane in its neighbourhood — the swimlane pane.
//!
//! The pane sits between the navigator and the PR body (ADR-0200) and shows
//! the repository's own history windowed around the PR: its commits, plus
//! [`CONTEXT_ROWS`] either side, with only the PR's own rows lit. A PR read in
//! isolation says nothing about where it branched from or what has landed
//! since; that is the whole point of a lane.
//!
//! It therefore reuses the rows the commit list already built — lanes,
//! colours and edges come from the one graph layout in `render`, so a lane
//! here is the *same* lane as in the main graph rather than a second opinion
//! about it. Nothing is laid out, fetched or read here.

use std::collections::HashSet;

use gpui::{div, prelude::*, px, relative, rgb, Context, SharedString};

use super::graph_view;
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// Row height, matching the navigator's meta line rather than the commit
/// list's 29px: the pane is a companion to the PR body, not a second history.
const ROW_H: f32 = 27.0;
/// How many commits of context to show either side of the PR's own.
const CONTEXT_ROWS: usize = 10;
/// How far a commit outside the PR is faded. It is context, not the subject.
const CONTEXT_OPACITY: f32 = 0.45;
/// Lanes drawn before the rail stops widening. A repository-wide graph can be
/// tens of lanes deep, and the pane is a companion pane, not the graph.
const MAX_RAIL_LANES: usize = 6;

/// The pane about the PR on screen: its lane in the repository's history, and
/// beneath that the detail (files, or checks and facts).
///
/// `None` only when no PR is on screen - Home keeps its tabs, so gating on
/// "any tab open" once left the previous PR's lane standing beside the home
/// list. When the PR's commits are not in the loaded history window there is
/// no neighbourhood to draw, and a lane alone would be a worse version of the
/// COMMITS tab: the detail then takes the whole pane.
pub(super) fn render_pr_lane(app: &KagiApp, cx: &mut Context<KagiApp>) -> Option<gpui::AnyElement> {
    let mode = app.pr_mode()?;
    let tab = mode.active.and_then(|ix| mode.tabs.get(ix))?;
    let number = tab.pr.number;
    let view = app.view();

    // The PR's commits, as positions in the history the commit list is showing.
    let mut hits: Vec<usize> = tab
        .commits
        .iter()
        .filter_map(|c| view.commit_row_index.get(&c.id).copied())
        .collect();
    hits.sort_unstable();
    let window = hits
        .first()
        .zip(hits.last())
        .and_then(|(first, last)| lane_window(*first, *last, view.rows.len()));
    let mine: HashSet<&kagi_git::CommitId> = tab.commits.iter().map(|c| &c.id).collect();

    // Lane numbers come from the repository-wide graph, where a PR's branch
    // can be lane 12 while the window only uses five lanes - and lane 12 is
    // off the rail whatever the rail's width (user report). The window's lanes
    // are therefore renumbered into consecutive columns, so "five lanes" means
    // five columns and nothing needed to be brought into view at all.
    let columns = match window {
        Some((lo, hi)) => {
            lane_columns((lo..=hi).filter_map(|ix| view.rows.get(ix)).map(|r| r.lane))
        }
        None => lane_columns(std::iter::empty()),
    };
    let lanes = columns.len().max(1);
    let rail = gutter_width(lanes);
    // The column the PR itself sits on - what the rail must keep in view when
    // there are more columns than fit.
    let pr_column = hits
        .iter()
        .filter_map(|ix| view.rows.get(*ix))
        .filter_map(|row| columns.get(&row.lane).copied())
        .min()
        .unwrap_or(0);
    let scroll = match mode.lane_scroll_x {
        Some(x) => x.clamp(0.0, max_scroll(lanes, rail)),
        None => follow_scroll(pr_column, rail),
    };
    // The commit list draws the node as the author's avatar in compact-lane
    // mode; the same setting means the same thing here.
    let avatars = theme::graph_lane_compact().then(|| app.avatars.images.clone());
    let lane_body = window.map(|(lo, hi)| {
        let mut body = div()
            .id("pr-lane-body")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for ix in lo..=hi {
            let Some(row) = view.rows.get(ix) else {
                continue;
            };
            body = body.child(render_lane_row(
                ix,
                row,
                number,
                mine.contains(&row.id),
                &Rail {
                    width: rail,
                    scroll,
                    columns: &columns,
                    avatars: avatars.as_ref(),
                },
                cx,
            ));
        }
        // Horizontal wheel/trackpad deltas scroll the rail; vertical ones are
        // left to the body's own scroll, exactly as the commit list's graph
        // column does.
        let scroll_by = cx.listener(
            move |this: &mut KagiApp, e: &gpui::ScrollWheelEvent, _w, cx| {
                this.pr_lane_scroll_by(&e.delta, lanes, rail, scroll, cx);
            },
        );
        body.on_scroll_wheel(scroll_by)
    });

    // The pane's lower third describes the PR: the files of the view, or the
    // checks and the facts about it. That used to be a pane of its own, which
    // only narrowed the diff the reader came for (ADR-0200, user report) -
    // this pane is already here, and a lane needs only the rows around the PR.
    // With no lane to draw, the detail is the whole pane rather than a third
    // of an empty one.
    let has_lane = lane_body.is_some();
    let detail = div()
        .id("pr-lane-detail")
        .when(has_lane, |el| {
            el.h(relative(1. / 3.))
                .flex_shrink_0()
                .border_t_1()
                .border_color(rgb(theme().surface))
        })
        .when(!has_lane, |el| el.flex_1().min_h(px(0.)))
        .bg(rgb(theme().panel))
        .child(super::pr_mode::render_pr_detail(app, tab, cx));

    Some(
        div()
            .id("pr-lane")
            .w(theme::scaled_px(rail + SUBJECT_W))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme().bg_base))
            .child(render_header(number, hits.len()))
            .children(lane_body)
            .child(detail)
            .into_any_element(),
    )
}

/// The rows to draw: the PR's own span, widened by [`CONTEXT_ROWS`] either
/// side and clipped to the history that is loaded.
///
/// `None` when there is no history to window — an empty list cannot show a
/// lane, and `rows - 1` would wrap.
fn lane_window(first: usize, last: usize, rows: usize) -> Option<(usize, usize)> {
    let last_row = rows.checked_sub(1)?;
    Some((
        first.saturating_sub(CONTEXT_ROWS),
        (last + CONTEXT_ROWS).min(last_row),
    ))
}

/// Width of the subject column beside the rail.
const SUBJECT_W: f32 = 240.0;

/// The rail's own width: the lanes in view, plus a half-lane of air so the
/// last line is not flush against the subject.
fn gutter_width(lanes: usize) -> f32 {
    graph_view::LANE_W * (lanes.clamp(1, MAX_RAIL_LANES) as f32 + 0.5)
}

/// Renumber the lanes a window actually uses into consecutive columns,
/// keeping their left-to-right order.
///
/// The commit list's lanes are repository-wide: a PR's branch can be lane 12
/// beside a mainline lane 0, with nothing in between. Drawing that literally
/// puts the PR twelve lanes out, past any rail worth the width. Columns are
/// the window's own answer — with five lanes in use there are five columns,
/// and nothing has to be scrolled into view.
fn lane_columns(lanes: impl Iterator<Item = usize>) -> std::collections::BTreeMap<usize, usize> {
    let used: std::collections::BTreeSet<usize> = lanes.collect();
    used.into_iter().zip(0..).collect()
}

/// A lane's column, or its own index when the window did not use it (an edge
/// may pass through a lane no row in the window sits on).
fn column_of(columns: &std::collections::BTreeMap<usize, usize>, lane: usize) -> usize {
    columns.get(&lane).copied().unwrap_or_else(|| {
        // Place it after everything the window did use, preserving order,
        // rather than colliding with a real column.
        columns
            .range(..lane)
            .next_back()
            .map(|(_, column)| column + 1)
            .unwrap_or(0)
    })
}

/// The furthest the rail can scroll: whatever of the lanes does not fit.
pub(super) fn max_scroll(lanes: usize, rail: f32) -> f32 {
    (lanes as f32 * graph_view::LANE_W - rail).max(0.0)
}

/// The scroll that brings `lane` into the rail, moving as little as possible.
///
/// A lane already in view scrolls nothing; one off the right edge scrolls just
/// far enough to seat it against that edge. This is what the pane does until
/// the reader scrolls it themselves.
pub(super) fn follow_scroll(lane: usize, rail: f32) -> f32 {
    let lane_w = graph_view::LANE_W;
    let left = lane as f32 * lane_w;
    let right = left + lane_w;
    if right > rail {
        right - rail
    } else {
        0.0
    }
}

/// The pane always has an active PR (see [`render_pr_lane`]), so the header
/// names it rather than asking whether there is one. The count is the PR's own
/// commits, not the windowed rows: the context is not part of the PR.
fn render_header(number: u64, commits: usize) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .h(theme::scaled_px(28.))
        .px_3()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_label))
        .child(SharedString::from(Msg::PrLaneTitle.t()))
        .child(
            div()
                .font_weight(gpui::FontWeight::NORMAL)
                .text_color(rgb(theme().color_branch))
                .child(SharedString::from(format!(
                    "#{number} \u{00B7} {commits} {}",
                    Msg::PrModeCommits.t()
                ))),
        )
}

/// What every row of the rail needs to know about the rail itself.
struct Rail<'a> {
    /// Rail width, the same on every row (that is what makes a lane a line).
    width: f32,
    /// Horizontal scroll, applied to the lane lines *and* the node together.
    scroll: f32,
    /// Global lane → column in this window (see [`lane_columns`]).
    columns: &'a std::collections::BTreeMap<usize, usize>,
    /// Author avatars when compact-lane mode draws nodes as avatars.
    avatars: Option<&'a std::collections::HashMap<String, std::sync::Arc<gpui::Image>>>,
}

/// One history row: its node on the rail, then the subject.
///
/// A row of the PR is lit and selects that commit in the PR; a context row is
/// faded and inert — clicking it would either navigate away from the PR being
/// read or pretend the commit is part of it.
fn render_lane_row(
    ix: usize,
    row: &super::commit_list::CommitRow,
    number: u64,
    is_mine: bool,
    rail: &Rail<'_>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let scroll = rail.scroll;
    let avatars = rail.avatars;
    let commit = row.id.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_lane_select(number, &commit, cx);
    });
    div()
        .id(("pr-lane-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(ROW_H))
        .when(!is_mine, |el| el.opacity(CONTEXT_OPACITY))
        .when(is_mine, |el| {
            el.cursor_pointer()
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(click)
        })
        .child({
            let lane_w = graph_view::lane_w();
            // Global lanes are renumbered into this window's columns, then the
            // rail's own scroll is subtracted — the same transform the canvas
            // below gets, so a line and its node are one lane.
            let column = column_of(rail.columns, row.lane);
            let node_cx = (column as f32) * lane_w + lane_w / 2.0 - scroll;
            let ring = theme::scaled(18.);
            let inner_d = theme::scaled(15.);
            let avatar = avatars.map(|images| {
                let inner = div()
                    .w(px(inner_d))
                    .h(px(inner_d))
                    .rounded_full()
                    .overflow_hidden();
                let inner = match images.get(&row.author_email).cloned() {
                    Some(image) => inner.child(
                        gpui::img(gpui::ImageSource::Image(image))
                            .size_full()
                            .rounded_full(),
                    ),
                    None => inner
                        .bg(kagi_ui_core::avatar::avatar_color(&row.author_email))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_color(gpui::white()).text_xs().child(
                            SharedString::from(kagi_ui_core::avatar::avatar_initial(&row.author)),
                        )),
                };
                div()
                    .absolute()
                    .left(px(node_cx - ring / 2.))
                    .top(px(theme::scaled(ROW_H) / 2. - ring / 2.))
                    .w(px(ring))
                    .h(px(ring))
                    .rounded_full()
                    .bg(theme().lane_color(row.node_color))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(inner)
            });
            div()
                // The rail is the same width on every row — that is what makes
                // a lane read as one line down the pane.
                .w(theme::scaled_px(rail.width))
                .h_full()
                .flex_shrink_0()
                .relative()
                .overflow_hidden()
                .child(
                    div().size_full().child(
                        graph_view::graph_canvas(
                            column,
                            row.node_color,
                            // Edges carry global lanes too, so they are mapped
                            // with the node — an edge left on lane 12 beside a
                            // node in column 1 is the line and the avatar
                            // coming apart (user report).
                            row.edges
                                .iter()
                                .map(|edge| kagi_domain::graph::GraphEdge {
                                    from_lane: column_of(rail.columns, edge.from_lane),
                                    to_lane: column_of(rail.columns, edge.to_lane),
                                    kind: edge.kind.clone(),
                                    color: edge.color,
                                })
                                .collect(),
                            row.is_head,
                            row.is_merge,
                            false,
                            // The canvas takes the same scroll the node is
                            // offset by, so a line and its node move together.
                            scroll,
                            0.,
                            Vec::new(),
                        )
                        .size_full(),
                    ),
                )
                .children(avatar)
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr_2()
                .truncate()
                .text_xs()
                .text_color(rgb(if is_mine {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .child(safe_text(&row.summary)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rail must not eat the subject at any lane depth, and must stop
    /// widening: a repository-wide graph is deeper than this pane is wide.
    #[test]
    fn the_rail_leaves_the_subject_room_to_read_and_stops_widening() {
        for lanes in [0, 1, 3, MAX_RAIL_LANES, 40] {
            let rail = gutter_width(lanes);
            assert!(rail > 0.0, "{lanes} lanes still need a rail");
            assert!(
                SUBJECT_W >= 200.0,
                "{lanes} lanes leave the subject {SUBJECT_W} px"
            );
        }
        assert_eq!(
            gutter_width(40),
            gutter_width(MAX_RAIL_LANES),
            "the rail is capped"
        );
        assert_eq!(
            gutter_width(0),
            gutter_width(1),
            "no lane is still one lane"
        );
    }

    /// The window is the PR's span plus context, clipped to what is loaded —
    /// never past the last row, and never wrapping when nothing is.
    #[test]
    fn the_window_adds_context_without_leaving_the_loaded_history() {
        assert_eq!(
            lane_window(40, 44, 200),
            Some((40 - CONTEXT_ROWS, 44 + CONTEXT_ROWS))
        );
        // At the head of history there is nothing above to show.
        assert_eq!(lane_window(0, 2, 200), Some((0, 2 + CONTEXT_ROWS)));
        // …and near its end, nothing below: the window clips, it does not
        // index past the rows it was given.
        assert_eq!(lane_window(190, 199, 200), Some((180, 199)));
        assert_eq!(lane_window(0, 0, 1), Some((0, 0)));
        assert_eq!(lane_window(0, 0, 0), None, "no history, no lane");
    }

    /// The reported case: the PR's branch is a high lane in the
    /// repository-wide graph, with nothing between it and the mainline. Once
    /// the window's lanes are columns it is the second column, not the
    /// twelfth, so no amount of rail width or scrolling was ever the fix.
    #[test]
    fn window_lanes_become_consecutive_columns() {
        let columns = lane_columns([0usize, 12, 12, 0, 3].into_iter());
        assert_eq!(columns.len(), 3, "three lanes in use, three columns");
        assert_eq!(column_of(&columns, 0), 0);
        assert_eq!(column_of(&columns, 3), 1);
        assert_eq!(column_of(&columns, 12), 2, "the PR's lane is reachable");

        // An edge may pass through a lane no row in the window sits on. It
        // lands after the last column below it rather than on top of one.
        assert_eq!(column_of(&columns, 7), 2);
        assert_eq!(column_of(&columns, 99), 3);
        assert_eq!(column_of(&lane_columns(std::iter::empty()), 5), 0);
    }

    /// The reported case: five lanes, the PR on the last one, a rail that
    /// fits fewer. Following must bring that lane into view — and must move
    /// nothing for a lane that is already there.
    #[test]
    fn following_brings_an_off_edge_lane_into_the_rail() {
        let lane_w = graph_view::LANE_W;
        let rail = lane_w * 2.5; // room for two lanes and a sliver
        assert_eq!(follow_scroll(0, rail), 0.0, "lane 0 is already in view");
        assert_eq!(follow_scroll(1, rail), 0.0, "so is lane 1");
        // Lane 4 of five sits past the right edge: scroll just enough to seat
        // it there, never further.
        let scrolled = follow_scroll(4, rail);
        assert_eq!(scrolled, 5.0 * lane_w - rail);
        assert!(scrolled > 0.0 && scrolled <= max_scroll(5, rail));
    }

    /// The rail cannot scroll past its lanes, and cannot scroll at all when
    /// they all fit.
    #[test]
    fn the_rail_stops_at_its_content() {
        let lane_w = graph_view::LANE_W;
        assert_eq!(max_scroll(1, lane_w * 2.5), 0.0, "nothing to reveal");
        assert_eq!(max_scroll(5, lane_w * 2.5), 5.0 * lane_w - lane_w * 2.5);
        assert_eq!(max_scroll(0, lane_w), 0.0, "no lanes, no scrolling");
    }
}
