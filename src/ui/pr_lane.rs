//! The PR swimlane pane — one lane per open PR tab, rows newest first.
//!
//! The pane sits between the navigator and the PR body (ADR-0200). Lanes and
//! row order come from `kagi_domain::pr_swimlane`, which is pure and tested;
//! the drawing reuses `graph_view::graph_canvas`, the painter the commit list
//! already uses, so a lane line and a node look the same here as there.
//!
//! Only commits the open tabs already carry are drawn: a tab holds its
//! `merge-base..head` range from the moment it opens, so the pane needs no
//! fetch of its own and no read on the render path.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_domain::graph::{EdgeKind, GraphEdge, NUM_COLORS};
use kagi_domain::pr_swimlane::{lay_out, LaneRow, Swimlane};

use super::graph_view;
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// Row height, matching the navigator's cards' meta line rather than the
/// commit list's 29px: the pane is a companion to the PR body, not a second
/// history.
const ROW_H: f32 = 27.0;
/// How far a row that belongs to another PR is faded (mock 1d, option A).
/// Recolouring the other lanes instead would need a second palette; fading the
/// row says "not this PR" with the colours already on screen.
const OTHER_LANE_OPACITY: f32 = 0.45;

/// The swimlane of every open PR tab, or `None` when no tab is open — the home
/// list is what the centre shows then, and an empty lane pane beside it would
/// be a column of nothing.
pub(super) fn render_pr_lane(app: &KagiApp, cx: &mut Context<KagiApp>) -> Option<gpui::AnyElement> {
    let mode = app.pr_mode()?;
    if mode.tabs.is_empty() {
        return None;
    }
    let lanes: Vec<(u64, &[kagi_git::Commit])> = mode
        .tabs
        .iter()
        .map(|tab| (tab.pr.number, tab.commits.as_slice()))
        .collect();
    let view = lay_out(&lanes);
    let active = mode
        .active
        .and_then(|ix| mode.tabs.get(ix))
        .map(|tab| tab.pr.number);
    let active_lane = active.and_then(|pr| view.lane_of(pr));
    let commits = active_lane
        .map(|lane| view.rows.iter().filter(|r| r.lane == lane).count())
        .unwrap_or(0);

    let mut body = div()
        .id("pr-lane-body")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (ix, row) in view.rows.iter().enumerate() {
        body = body.child(render_lane_row(ix, row, &view, active_lane, cx));
    }

    Some(
        div()
            .id("pr-lane")
            .w(theme::scaled_px(lane_pane_width(view.lanes.len())))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme().bg_base))
            .child(render_header(active, commits))
            .child(body)
            .into_any_element(),
    )
}

/// Wide enough for the lanes plus a readable slice of the subject. One lane is
/// the common case (one PR open) and must not leave the subject in a sliver.
fn lane_pane_width(lanes: usize) -> f32 {
    let rail = gutter_width(lanes);
    (rail + 260.0).min(520.0)
}

/// The lane rail's own width: every lane, plus a half-lane of air so the last
/// line is not flush against the subject.
fn gutter_width(lanes: usize) -> f32 {
    graph_view::LANE_W * (lanes.max(1) as f32 + 0.5)
}

fn render_header(active: Option<u64>, commits: usize) -> gpui::Div {
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
        .children(active.map(|pr| {
            div()
                .font_weight(gpui::FontWeight::NORMAL)
                .text_color(rgb(theme().color_branch))
                .child(SharedString::from(format!(
                    "#{pr} \u{00B7} {commits} {}",
                    Msg::PrModeCommits.t()
                )))
        }))
}

/// One commit: its lane's node on the rail, then the subject.
fn render_lane_row(
    ix: usize,
    row: &LaneRow,
    view: &Swimlane,
    active_lane: Option<usize>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let is_active = active_lane == Some(row.lane);
    // Every lane keeps its line through every row, so a lane reads as one
    // continuous thread rather than as a dotted trail of its own commits.
    let edges: Vec<GraphEdge> = (0..view.lanes.len())
        .map(|lane| GraphEdge {
            from_lane: lane,
            to_lane: lane,
            kind: EdgeKind::Pass,
            color: lane % NUM_COLORS,
        })
        .collect();
    let commit = row.commit.clone();
    let pr = view.lanes.get(row.lane).copied().unwrap_or_default();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_lane_select(pr, &commit, cx);
    });
    div()
        .id(("pr-lane-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(ROW_H))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .when(!is_active, |el| el.opacity(OTHER_LANE_OPACITY))
        .child(
            div()
                .w(theme::scaled_px(gutter_width(view.lanes.len())))
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .child(
                    div().size_full().child(
                        graph_view::graph_canvas(
                            row.lane,
                            row.lane % NUM_COLORS,
                            edges,
                            // The tip of a lane is that PR's head — the commit a
                            // merge would take, which is what `is_head` marks in
                            // the commit list too.
                            row.is_tip,
                            false,
                            false,
                            0.,
                            0.,
                            Vec::new(),
                        )
                        .size_full(),
                    ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr_2()
                .truncate()
                .text_xs()
                .text_color(rgb(if is_active {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .child(safe_text(&row.subject)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One PR open is the common case: the rail must not eat the subject, and
    /// many lanes must not push the pane past the centre pane's share.
    #[test]
    fn the_pane_leaves_room_for_the_subject_at_every_lane_count() {
        let one = lane_pane_width(1);
        assert!(
            one - gutter_width(1) >= 200.0,
            "one lane leaves {one} px, rail {}",
            gutter_width(1)
        );
        assert!(lane_pane_width(40) <= 520.0, "capped for a deep stack");
        assert!(lane_pane_width(4) > one, "more lanes, more rail");
        // Zero lanes cannot be rendered (the pane is `None`), but the width
        // helper must still not return a rail of nothing.
        assert_eq!(gutter_width(0), gutter_width(1));
    }
}
