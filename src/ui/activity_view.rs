//! Bottom-panel "Activity" tab presentation — commit-activity line chart and
//! contributor ranking. Pure view helpers (no `KagiApp` state, no git): the
//! interactive bits (granularity toggle) live in `render.rs`, which feeds the
//! already-aggregated [`kagi_domain::activity`] data in here.

use gpui::{
    canvas, div, point, px, App, Bounds, Canvas, InteractiveElement as _, IntoElement,
    ParentElement as _, PathBuilder, Pixels, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};
use gpui_component::tooltip::Tooltip;

use kagi_domain::activity::{ActivityBucket, Contributor};

use crate::ui::theme::{self, theme};

/// A `canvas` that paints two polylines over the bucket series: total commits
/// (accent colour) and, on top, merge commits (warning colour). A baseline is
/// drawn along the bottom; the y-axis is scaled to the max commit count.
pub fn activity_chart(buckets: Vec<ActivityBucket>) -> Canvas<()> {
    canvas(
        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |bounds: Bounds<Pixels>, _prepaint: (), window: &mut Window, _cx: &mut App| {
            let ox = f32::from(bounds.origin.x);
            let oy = f32::from(bounds.origin.y);
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);

            let pad = theme::scaled(10.0);
            let plot_l = ox + pad;
            let plot_r = ox + w - pad;
            let plot_t = oy + pad;
            let plot_b = oy + h - pad;
            let plot_w = (plot_r - plot_l).max(1.0);
            let plot_h = (plot_b - plot_t).max(1.0);

            let n = buckets.len();
            if n == 0 {
                return;
            }
            let max_c = buckets.iter().map(|b| b.commits).max().unwrap_or(1).max(1) as f32;

            let x_at = |i: usize| -> f32 {
                if n == 1 {
                    (plot_l + plot_r) / 2.0
                } else {
                    plot_l + (i as f32) * (plot_w / (n as f32 - 1.0))
                }
            };
            let y_at = |v: u32| -> f32 { plot_b - (v as f32 / max_c) * plot_h };

            // Baseline along the bottom.
            let mut base = PathBuilder::stroke(theme::scaled_px(1.0));
            base.move_to(point(px(plot_l), px(plot_b)));
            base.line_to(point(px(plot_r), px(plot_b)));
            if let Ok(p) = base.build() {
                window.paint_path(p, hsla(theme().text_muted));
            }

            // Polyline helper: stroke through (i, value) for each bucket.
            let mut polyline =
                |value: &dyn Fn(&ActivityBucket) -> u32, width: f32, color: gpui::Hsla| {
                    let mut b = PathBuilder::stroke(theme::scaled_px(width));
                    for (i, bucket) in buckets.iter().enumerate() {
                        let p = point(px(x_at(i)), px(y_at(value(bucket))));
                        if i == 0 {
                            b.move_to(p);
                        } else {
                            b.line_to(p);
                        }
                    }
                    if let Ok(path) = b.build() {
                        window.paint_path(path, color);
                    }
                };

            // Commits (green) first, merges (pink) on top so both read.
            polyline(&|b| b.commits, 1.8, hsla(theme().color_success));
            polyline(&|b| b.merges, 1.5, hsla(theme().color_head));
        },
    )
}

/// `0xRRGGBB` → opaque [`gpui::Hsla`] (what `paint_path` takes).
#[inline]
fn hsla(hex: u32) -> gpui::Hsla {
    gpui::rgb(hex).into()
}

/// A transparent overlay of one interactive column per bucket: hovering a column
/// highlights that slice and shows a tooltip with its time + commit/merge count.
/// Sits absolutely over the chart canvas (no canvas hit-testing needed).
pub fn hover_overlay(
    buckets: &[ActivityBucket],
    start: i64,
    bucket_secs: i64,
    now: i64,
) -> impl IntoElement {
    let dark = theme().dark;
    let hover_bg = if dark {
        gpui::hsla(0., 0., 1., 0.07)
    } else {
        gpui::hsla(0., 0., 0., 0.06)
    };
    let mut row = div().absolute().inset_0().flex().flex_row();
    for (i, b) in buckets.iter().enumerate() {
        let center = start + (i as i64) * bucket_secs + bucket_secs / 2;
        let ago = crate::ui::commit_list::relative_time(center, now);
        let tip = SharedString::from(if b.commits == 0 {
            format!("{ago} · no commits")
        } else {
            format!("{ago} · {} commits · {} merges", b.commits, b.merges)
        });
        row = row.child(
            div()
                .id(SharedString::from(format!("activity-bucket-{i}")))
                .flex_1()
                .h_full()
                .hover(move |s| s.bg(hover_bg))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
        );
    }
    row
}

/// One compact contributor ranking row: `#rank  name … commits·merges` with a
/// thin commit bar beneath, proportional to the leader's commit count.
pub fn contributor_row(rank: usize, c: &Contributor, max_commits: u32) -> impl IntoElement {
    let bar_frac = if max_commits > 0 {
        (c.commits as f32 / max_commits as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .py(theme::scaled_px(1.5))
        .child(
            div()
                .w(theme::scaled_px(14.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{rank}"))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(theme::scaled_px(1.))
                .child(
                    // Name (left) + counts (right) on one line.
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_sm()
                                .text_color(rgb(theme().text_main))
                                .truncate()
                                .child(SharedString::from(c.name.clone())),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!(
                                    "{} · {} merges",
                                    c.commits, c.merges
                                ))),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .h(theme::scaled_px(3.))
                        .rounded_full()
                        .bg(rgb(theme().surface))
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(bar_frac))
                                .rounded_full()
                                .bg(rgb(theme().color_success)),
                        ),
                ),
        )
}

#[inline]
fn rgb(hex: u32) -> gpui::Rgba {
    gpui::rgb(hex)
}
