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

use gpui::{div, prelude::*, px, relative, rgb, Context, SharedString};

use super::graph_view;
#[cfg(test)]
use super::graph_window::{column_of, is_local_connector};
use super::graph_window::{
    follow_scroll, gutter_width, lane_columns, max_scroll, render_rail, Rail, CONTEXT_OPACITY,
    MAX_RAIL_LANES,
};
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

#[derive(PartialEq, Eq)]
struct WindowKey {
    pr: kagi_domain::github::PrKey,
    span: (usize, usize),
    root: Option<usize>,
}

/// Session-owned vertical scroll and stable-key row focus for the companion.
pub(super) struct Navigation {
    pub(super) keyboard: super::list_keyboard::ListKeyboard,
    scroll: gpui::ListState,
    window: std::cell::RefCell<Option<WindowKey>>,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            keyboard: Default::default(),
            scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(0.)),
            window: Default::default(),
        }
    }
}

/// Row height, matching the navigator's meta line rather than the commit
/// list's 29px: the pane is a companion to the PR body, not a second history.
const ROW_H: f32 = 27.0;
/// How many commits of context to show either side of the PR's own.
const CONTEXT_ROWS: usize = 10;

/// The pane about the PR on screen: its lane in the repository's history, and
/// beneath that the detail (files, or checks and facts).
///
/// `None` only when no PR is on screen - Home keeps its tabs, so gating on
/// "any tab open" once left the previous PR's lane standing beside the home
/// list. When the PR's commits are not in the loaded history window there is
/// no neighbourhood to draw, and a lane alone would be a worse version of the
/// COMMITS tab: the detail then takes the whole pane.
pub(super) fn render_pr_lane(
    app: &KagiApp,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let mode = app.pr_mode()?;
    let tab = mode.active.and_then(|ix| mode.tabs.get(ix))?;
    let number = tab.pr.number;
    // One shared key for every row's click: a number alone could name the
    // same-numbered PR of another repository open in this session.
    let key = std::rc::Rc::new(tab.pr.key());
    let local_refs_loading = tab.local_refs_loading;
    let view = app.view();

    // The PR's commits, as positions in the history the commit list is showing.
    let mut hits: Vec<usize> = tab
        .commits
        .iter()
        .filter_map(|c| view.commit_row_index.get(&c.id).copied())
        .collect();
    hits.sort_unstable();
    let span = hits
        .first()
        .zip(hits.last())
        .and_then(|(first, last)| lane_window(*first, *last, view.rows.len()));

    // Lane numbers come from the repository-wide graph, where a PR's branch
    // can be lane 12 while the window only uses five lanes - and lane 12 is
    // off the rail whatever the rail's width (user report). The window's lanes
    // are therefore renumbered into consecutive columns, so "five lanes" means
    // five columns and nothing needed to be brought into view at all.
    // The commit the branch grew from - the PR's merge-base - drawn under
    // the window even when it is further than the context rows reach, behind
    // an elision row saying how many commits were skipped (user request). A
    // base inside the window is simply one of its rows; one not loaded at all
    // cannot be drawn.
    let root: Option<usize> = span.and_then(|(_, hi)| {
        root_row(
            hi,
            view.commit_row_index.get(&tab.base).copied(),
            view.rows.len(),
        )
    });
    let columns = match span {
        Some((lo, hi)) => lane_columns(
            (lo..=hi)
                .chain(root)
                .filter_map(|ix| view.rows.get(ix))
                .map(|r| r.lane),
        ),
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
        Some(x) => x.clamp(0.0, max_scroll(lanes, rail, graph_view::LANE_W)),
        None => follow_scroll(pr_column, rail, graph_view::LANE_W),
    };
    let active = app.window_active_for_render(window);
    let selected = tab
        .selected_commit
        .and_then(|ix| tab.commits.get(ix))
        .map(|c| c.id.clone());
    let heading = lane_heading(number, tab.pr.commit_total, hits.len());
    let lane_body = LaneBody {
        key,
        span,
        root,
        hits,
        columns,
        rail,
        scroll,
        selected,
        active,
        heading: heading.clone(),
    }
    .render(app, window, cx);

    // The pane's lower third lists the files of a file view. The checks and
    // the properties left it for the PR's own page (mock 7a), so on every
    // other view there is nothing to put here and the lane takes the height
    // back rather than keeping an empty third.
    let has_lane = lane_body.is_some();
    let detail = super::pr_mode::render_pr_detail(app, cx).map(|body| {
        div()
            .id("pr-lane-detail")
            .when(has_lane, |el| {
                el.h(relative(1. / 3.))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(theme().surface))
            })
            .when(!has_lane, |el| el.flex_1().min_h(px(0.)))
            .bg(rgb(theme().panel))
            .child(body)
    });

    Some(
        div()
            .id("pr-lane")
            .w(theme::scaled_px(rail + SUBJECT_W))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme().bg_base))
            .child(render_header(heading))
            .when(local_refs_loading, |lane| {
                lane.child(
                    div()
                        .id("pr-lane-loading")
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(super::pr_conversation::render_loading()),
                )
            })
            .children(lane_body)
            .children(detail)
            .into_any_element(),
    )
}

struct LaneBody {
    key: std::rc::Rc<kagi_domain::github::PrKey>,
    span: Option<(usize, usize)>,
    root: Option<usize>,
    hits: Vec<usize>,
    columns: std::collections::BTreeMap<usize, usize>,
    rail: f32,
    scroll: f32,
    selected: Option<kagi_git::CommitId>,
    active: bool,
    heading: String,
}

impl LaneBody {
    fn render(
        self,
        app: &KagiApp,
        window: &mut gpui::Window,
        cx: &mut Context<KagiApp>,
    ) -> Option<gpui::AnyElement> {
        let nav = &app.pr_mode()?.lane_navigation;
        let Some((lo, hi)) = self.span else {
            nav.keyboard
                .yield_focus(app.root_focus.as_ref(), window, cx);
            return None;
        };
        let signature = WindowKey {
            pr: self.key.as_ref().clone(),
            span: (lo, hi),
            root: self.root,
        };
        if nav.window.borrow().as_ref() != Some(&signature) {
            nav.scroll
                .reset(hi - lo + 1 + usize::from(self.root.is_some()) * 2);
            *nav.window.borrow_mut() = Some(signature);
        }
        let scroll = nav
            .scroll
            .clone()
            .with_uniform_item_height(theme::scaled_px(ROW_H));
        let rows = nav.keyboard.rows(
            self.hits
                .iter()
                .map(|ix| (app.view().rows[*ix].id.0.as_str(), ix - lo)),
            &scroll,
            app.root_focus.as_ref(),
            window,
            cx,
        );
        let label = format!("{} {}", Msg::PrLaneTitle.t(), self.heading);
        let frame = super::list_a11y::list_box_named(
            "pr-lane-body",
            rows.list(div().id("pr-lane-body")),
            label.into(),
        )
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
        let lanes = self.columns.len().max(1);
        let width = self.rail;
        let horizontal = self.scroll;
        let render = cx.processor(move |app: &mut KagiApp, entry: usize, _, cx| {
            let ix = if entry <= hi - lo {
                lo + entry
            } else if entry == hi - lo + 1 {
                return render_elision_row(self.root.unwrap() - hi - 1, self.rail);
            } else {
                self.root.unwrap()
            };
            let row = &app.view().rows[ix];
            let slot = self.hits.binary_search(&ix).ok();
            let rail = Rail {
                width: self.rail,
                scroll: self.scroll,
                columns: &self.columns,
                avatars: theme::graph_lane_compact().then_some(app.avatars.images.as_ref()),
            };
            render_lane_row(
                ix,
                row,
                &self.key,
                (slot, self.selected.as_ref() == Some(&row.id), self.active),
                &rows,
                &rail,
                cx,
            )
        });
        Some(
            frame
                .child(
                    gpui::list(scroll, move |ix, window, cx| render(ix, window, cx))
                        .flex_1()
                        .min_h(px(0.)),
                )
                .child(super::e2e::measure_inside("pr-lane-body"))
                .on_scroll_wheel(cx.listener(
                    move |app: &mut KagiApp, e: &gpui::ScrollWheelEvent, _, cx| {
                        app.pr_lane_scroll_by(&e.delta, lanes, width, horizontal, cx);
                    },
                ))
                .into_any_element(),
        )
    }
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
/// Conservative bound from the existing graph rail, without rebuilding lanes
/// just to decide whether the companion fits beside the reader.
pub(super) const MAX_PANE_WIDTH: f32 =
    graph_view::LANE_W * (MAX_RAIL_LANES as f32 + 0.5) + SUBJECT_W;

/// The heading explicitly distinguishes an authoritative total from loaded hits.
fn lane_heading(number: u64, total: Option<usize>, loaded: usize) -> String {
    let count = match total {
        Some(total) => format!("{total} {}", Msg::PrModeCommits.t()),
        None => Msg::PrLaneLoaded.t().replacen("{}", &loaded.to_string(), 1),
    };
    format!("#{number} \u{00B7} {count}")
}

fn render_header(heading: String) -> gpui::Div {
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
                .child(SharedString::from(heading)),
        )
}

/// The row to draw as the branch's root under the window: the merge-base's
/// row when it lies below the window's last row `hi` and inside the loaded
/// history. Inside the window it is already drawn; unloaded it cannot be.
fn root_row(hi: usize, base: Option<usize>, rows: usize) -> Option<usize> {
    base.filter(|ix| *ix > hi && *ix < rows)
}

/// The gap between the window and the PR's root: a dotted rule across the
/// rail and the count of commits it stands for, at row height so the root
/// under it reads as "the same column, further down".
fn render_elision_row(skipped: usize, rail: f32) -> gpui::AnyElement {
    div()
        .id("pr-lane-elision")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(ROW_H))
        .child(
            div()
                .w(theme::scaled_px(rail))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("\u{22ee}")),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(super::i18n::pr_lane_elided(skipped))),
        )
        .into_any_element()
}

fn lane_row_label(subject: &str, sha: &str, number: u64, is_mine: bool) -> String {
    let template = if is_mine {
        Msg::PrLaneCommit
    } else {
        Msg::PrLaneContext
    };
    let number = number.to_string();
    let args = [subject, sha, number.as_str()];
    let mut parts = template.t().split("{}");
    let mut label = parts.next().unwrap_or_default().to_owned();
    for (ix, (arg, part)) in args.iter().zip(parts).enumerate() {
        if ix == 0 {
            kagi_domain::text_safety::append_sanitized_control_bytes(&mut label, arg);
        } else {
            label.push_str(arg);
        }
        label.push_str(part);
    }
    label
}

/// One history row: its node on the rail, then the subject.
///
/// A row of the PR is lit and selects that commit in the PR; a context row is
/// faded and inert — clicking it would either navigate away from the PR being
/// read or pretend the commit is part of it.
fn render_lane_row(
    ix: usize,
    row: &super::commit_list::CommitRow,
    pr: &std::rc::Rc<kagi_domain::github::PrKey>,
    (slot, selected, active): (Option<usize>, bool, bool),
    rows: &std::rc::Rc<super::keyboard_nav::RowList>,
    rail: &Rail<'_>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let is_mine = slot.is_some();
    let label = lane_row_label(&row.summary, &row.id.0, pr.number, is_mine);
    let el = div().id(("pr-lane-row", ix));
    let el = if let Some(slot) = slot {
        let commit = row.id.clone();
        let pr = pr.clone();
        let focus = rows.clone();
        super::list_a11y::list_option(
            "pr-lane-body",
            rows.row(slot, el),
            slot,
            rows.len(),
            label,
            selected,
        )
        .cursor_pointer()
        .when(!selected, |el| {
            el.hover(|style| style.bg(rgb(theme().surface)))
        })
        .when(selected, |el| el.bg(theme().selected_for_window(active)))
        .on_click(cx.listener(move |app: &mut KagiApp, _, window, cx| {
            focus.focus_row(slot, window, cx);
            app.pr_lane_select(&pr, &commit, cx);
        }))
    } else {
        el.role(gpui::Role::ListItem)
            .aria_label(SharedString::from(label))
            .opacity(CONTEXT_OPACITY)
    };
    let el = el
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(ROW_H))
        .child(
            render_rail(
                row.lane,
                row.node_color,
                &row.edges,
                graph_view::GraphNode::Commit {
                    is_head: row.is_head,
                    is_merge: row.is_merge,
                },
                Some((&row.author_email, &row.author)),
                ROW_H,
                rail,
            )
            .when(is_mine, |el| el.ml(px(-super::keyboard_nav::RING))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr_2()
                .truncate()
                .text_xs()
                .relative()
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(super::e2e::measure_inside(format!("pr-lane-label-{ix}")))
                })
                .text_color(if is_mine && selected {
                    theme().selection_foreground(active)
                } else {
                    rgb(if is_mine {
                        theme().text_main
                    } else {
                        theme().text_muted
                    })
                    .into()
                })
                .child(safe_text(&row.summary)),
        );
    #[cfg(feature = "gui-e2e")]
    let el = super::e2e::measure_control(format!("pr-lane-row-{ix}"), el);
    el.into_any_element()
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn pr_lane_history_for_e2e(&self) -> Vec<super::commit_list::CommitRow> {
        self.view().rows.clone()
    }

    pub fn set_pr_lane_total_for_e2e(&mut self, total: Option<usize>, cx: &mut Context<Self>) {
        if let Some(mode) = self.pr_mode_mut() {
            if let Some(tab) = mode.active.and_then(|ix| mode.tabs.get_mut(ix)) {
                tab.pr.commit_total = total;
            }
        }
        cx.notify();
    }

    pub fn set_pr_lane_history_for_e2e(
        &mut self,
        rows: Vec<super::commit_list::CommitRow>,
        cx: &mut Context<Self>,
    ) {
        let index = rows
            .iter()
            .enumerate()
            .map(|(ix, row)| (row.id.clone(), ix))
            .collect();
        self.view_mut().rows = rows;
        self.view_mut().commit_row_index = index;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lane_names_keep_literal_braces_and_neutralize_control_bytes() {
        let name = lane_row_label("Fix {} \u{001b} subject", "fullsha", 7, true);
        assert!(name.contains("Fix {}"));
        assert!(name.contains("fullsha"));
        assert!(name.contains("PR #7"));
        assert!(!name.contains('\u{001b}'));
        let context = lane_row_label("Context", "othersha", 7, false);
        assert!(context.contains("othersha"));
        assert!(!context.contains("PR #7"));
    }

    #[test]
    fn explicit_total_is_independent_of_loaded_history() {
        assert_eq!(lane_heading(7, Some(10), 3), lane_heading(7, Some(10), 10));
        assert_ne!(lane_heading(7, None, 3), lane_heading(7, Some(3), 3));
        assert_ne!(lane_heading(7, None, 3), lane_heading(7, None, 10));
    }

    /// The branch root is drawn under the window only when it is actually
    /// further down than the context reaches and is loaded at all.
    #[test]
    fn the_root_is_drawn_only_beyond_the_window_and_within_history() {
        assert_eq!(root_row(20, Some(45), 200), Some(45), "past the window");
        assert_eq!(
            root_row(20, Some(15), 200),
            None,
            "inside the window: already a row"
        );
        assert_eq!(
            root_row(20, Some(20), 200),
            None,
            "the window's own last row"
        );
        assert_eq!(root_row(20, Some(250), 200), None, "not loaded");
        assert_eq!(root_row(20, None, 200), None, "unknown base");
    }

    /// The reported dashed line beside every subject: the main graph's
    /// WIP→HEAD connector (and a squash ghost link) rode the row's edges into
    /// the lane, where the column remap put it after the last real column.
    /// Both are local-working-tree edges and must not reach the lane; an
    /// ordinary parent edge must.
    #[test]
    fn local_connectors_stay_out_of_the_lane() {
        use kagi_domain::graph::{EdgeKind, GraphEdge};
        let edge = |color: usize| GraphEdge {
            from_lane: 3,
            to_lane: 3,
            kind: EdgeKind::Pass,
            color,
        };
        assert!(is_local_connector(&edge(
            super::super::graph_wip::wip_color(2)
        )));
        assert!(is_local_connector(&edge(
            super::super::graph_squash::GHOST_COLOR
        )));
        assert!(!is_local_connector(&edge(2)));
    }

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
        assert_eq!(
            follow_scroll(0, rail, lane_w),
            0.0,
            "lane 0 is already in view"
        );
        assert_eq!(follow_scroll(1, rail, lane_w), 0.0, "so is lane 1");
        // Lane 4 of five sits past the right edge: scroll just enough to seat
        // it there, never further.
        let scrolled = follow_scroll(4, rail, lane_w);
        assert_eq!(scrolled, 5.0 * lane_w - rail);
        assert!(scrolled > 0.0 && scrolled <= max_scroll(5, rail, lane_w));
    }

    /// The rail cannot scroll past its lanes, and cannot scroll at all when
    /// they all fit.
    #[test]
    fn the_rail_stops_at_its_content() {
        let lane_w = graph_view::LANE_W;
        assert_eq!(
            max_scroll(1, lane_w * 2.5, lane_w),
            0.0,
            "nothing to reveal"
        );
        assert_eq!(
            max_scroll(5, lane_w * 2.5, lane_w),
            5.0 * lane_w - lane_w * 2.5
        );
        assert_eq!(max_scroll(0, lane_w, lane_w), 0.0, "no lanes, no scrolling");
    }
}
