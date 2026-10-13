//! Activity presentation: the existing aggregate, chart, period selector and ranking.
use super::{
    activity_chart::{activity_chart, paint_series},
    commit_list, e2e, keyboard_nav,
    theme::{self, theme},
    KagiApp,
};
use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, Bounds, Context, Pixels, Role, SharedString,
    Window,
};
use kagi_domain::activity::{Contributor, Granularity, GranularityData};
use kagi_ui_core::i18n::{activity as activity_text, Msg};

fn legend(merges: bool, text: String) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .items_center()
        .gap_1()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds: Bounds<Pixels>, (), window, _| {
                    paint_series(
                        [
                            point(bounds.left(), bounds.center().y),
                            point(bounds.right(), bounds.center().y),
                        ],
                        merges,
                        window,
                        "legend",
                    );
                },
            )
            .w(theme::scaled_px(18.))
            .h(theme::scaled_px(4.)),
        )
        .child(SharedString::from(text))
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(e2e::measure_inside(if merges {
                "activity-legend-merges"
            } else {
                "activity-legend-commits"
            }))
        })
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

impl KagiApp {
    pub(super) fn render_activity_body(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(feature = "gui-e2e")]
        e2e::activity::begin();
        let data = self.view().activity.get(self.activity_granularity);
        if data.total_commits == 0 || data.buckets.is_empty() {
            self.activity_bucket_focus.sync(&[], cx);
        }
        self.activity_bucket_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let chart = if data.total_commits == 0 || data.buckets.is_empty() {
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(format!(
                    "No commits in the {}",
                    self.activity_granularity.window_label()
                ))
                .into_any_element()
        } else {
            self.activity_plot(data, window, cx)
        };
        div()
            .relative()
            .bg(rgb(theme().panel))
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap(theme::scaled_px(4.))
            .p(theme::scaled_px(6.))
            .child(self.activity_header(data, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .flex()
                    .gap(theme::scaled_px(8.))
                    .child(chart)
                    .child(self.activity_ranking(data, cx)),
            )
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(e2e::measure_inside("activity-body"))
            })
            .into_any_element()
    }

    fn activity_periods(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.weak_entity();
        let selected = Granularity::ALL
            .iter()
            .position(|g| *g == self.activity_granularity);
        let tabs = keyboard_nav::TabList::new(
            &self.activity_period_focus,
            keyboard_nav::slot_keys(Granularity::ALL.len()),
            (0..Granularity::ALL.len()).collect(),
            selected,
            keyboard_nav::Activation::Automatic,
            self.root_focus.clone(),
            move |slot, _, _, cx| {
                let _ = entity.update(cx, |app, cx| {
                    app.activity_granularity = Granularity::ALL[slot];
                    app.activity_hover = None;
                    cx.notify();
                });
            },
            cx,
        );
        let mut row = tabs
            .list(
                div()
                    .id("activity-periods")
                    .relative()
                    .flex()
                    .items_center()
                    .gap_1(),
            )
            .aria_label(Msg::ActivityPeriod.t());
        for (slot, gran) in Granularity::ALL.iter().enumerate() {
            let active = Some(slot) == selected;
            let id = format!("activity-gran-{}", gran.label());
            let label = period_label(*gran);
            let cell = period_cell(&tabs, slot, &id, label, active);
            #[cfg(feature = "gui-e2e")]
            {
                let attributes = self.activity_period_focus.recorded_tabs();
                e2e::activity::record(
                    &id,
                    e2e::activity::Node {
                        role: Some(Role::Tab),
                        name: label.into(),
                        selected: active,
                        tab_stop: attributes[slot].3,
                        focus: Some(tabs.focus_handle(slot).clone()),
                    },
                );
            }
            row = row.child(cell);
        }
        #[cfg(feature = "gui-e2e")]
        e2e::activity::record(
            "activity-periods",
            e2e::activity::Node {
                role: Some(Role::TabList),
                name: Msg::ActivityPeriod.t().into(),
                selected: false,
                tab_stop: false,
                focus: None,
            },
        );
        row.when(cfg!(feature = "gui-e2e"), |row| {
            row.child(e2e::measure_inside("activity-periods"))
        })
    }

    fn activity_header(
        &self,
        data: &GranularityData,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focus = self.activity_bucket_focus.focused(window);
        let inspected = if window.last_input_was_keyboard() {
            focus.or(self.activity_hover)
        } else {
            self.activity_hover.or(focus)
        }
        .filter(|&i| i < data.buckets.len());
        let (label, commits, merges) = match inspected {
            Some(i) => {
                let center = data.start + i as i64 * data.bucket_secs + data.bucket_secs / 2;
                (
                    format!("at {}", commit_list::relative_time(center, data.now)),
                    data.buckets[i].commits,
                    data.buckets[i].merges,
                )
            }
            None => (
                self.activity_granularity.window_label().to_string(),
                data.total_commits,
                data.total_merges,
            ),
        };
        #[cfg(feature = "gui-e2e")]
        e2e::activity::record_readout(&format!("{label}: {commits} commits, {merges} merges"));
        div()
            .relative()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(theme::scaled_px(4.))
            .flex_shrink_0()
            .min_w(px(0.))
            .child(self.activity_periods(cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(label))
                    .child(legend(false, format!("{commits} commits")))
                    .child(legend(true, format!("{merges} merges")))
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.child(e2e::measure_inside("activity-readout"))
                    }),
            )
    }

    fn activity_plot(
        &self,
        data: &GranularityData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tick = |text: String| {
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(text)
        };
        let axis_w = theme::scaled_px(26.);
        div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(
                        div()
                            .relative()
                            .w(axis_w)
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .items_end()
                            .pr_1()
                            .py(theme::scaled_px(6.))
                            .child(tick(
                                data.buckets
                                    .iter()
                                    .map(|b| b.commits)
                                    .max()
                                    .unwrap_or(0)
                                    .to_string(),
                            ))
                            .child(tick("0".into()))
                            .when(cfg!(feature = "gui-e2e"), |el| {
                                el.child(e2e::measure_inside("activity-y-axis"))
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(px(0.))
                            .child(activity_chart(data.buckets.clone()).size_full())
                            .child(self.activity_bucket_overlay(data, window, cx))
                            .when(cfg!(feature = "gui-e2e"), |el| {
                                el.child(e2e::measure_inside("activity-chart"))
                            }),
                    ),
            )
            .child(
                div().flex().child(div().w(axis_w).flex_shrink_0()).child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .justify_between()
                        .child(tick(data.start_label.clone()))
                        .child(tick("now".into()))
                        .when(cfg!(feature = "gui-e2e"), |el| {
                            el.child(e2e::measure_inside("activity-x-axis"))
                        }),
                ),
            )
            .into_any_element()
    }

    fn activity_bucket_overlay(
        &self,
        data: &GranularityData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let handles = std::rc::Rc::new(
            self.activity_bucket_focus
                .sync(&keyboard_nav::slot_keys(data.buckets.len()), cx),
        );
        self.activity_bucket_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let stop = self.activity_bucket_focus.focused(window).unwrap_or(0);
        let hover_bg = gpui::hsla(
            0.,
            0.,
            if theme().dark { 1. } else { 0. },
            if theme().dark { 0.07 } else { 0.06 },
        );
        let mut overlay = div()
            .id("activity-buckets")
            .absolute()
            .inset_0()
            .flex()
            .role(Role::Group)
            .aria_label(Msg::ActivityChart.t())
            .key_context("KagiTabList KagiActivityBuckets");
        for (i, bucket) in data.buckets.iter().enumerate() {
            let id = format!("activity-bucket-{i}");
            let name = activity_text::bucket(
                &data.bucket_date(i),
                bucket.commits,
                bucket.merges,
                bucket.authors,
            );
            let handle = handles[i].clone().tab_index(0).tab_stop(i == stop);
            #[cfg(feature = "gui-e2e")]
            e2e::activity::record(
                &id,
                e2e::activity::Node {
                    role: Some(Role::Status),
                    name: name.clone(),
                    selected: false,
                    tab_stop: i == stop,
                    focus: Some(handle.clone()),
                },
            );
            let cell = keyboard_nav::with_ring(
                div()
                    .id(SharedString::from(id.clone()))
                    .relative()
                    .role(Role::Status)
                    .aria_label(SharedString::from(name))
                    .track_focus(&handle),
            )
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .hover(move |s| s.bg(hover_bg))
            .on_hover(cx.listener(move |app, hovered: &bool, _, cx| {
                if *hovered {
                    app.activity_hover = Some(i);
                } else if app.activity_hover == Some(i) {
                    app.activity_hover = None;
                }
                cx.notify();
            }))
            .on_key_down(keyboard_nav::stop_activation_keys);
            let cell = bucket_keys(cell, i, handles.clone(), cx)
                .when(cfg!(feature = "gui-e2e"), |cell| {
                    cell.child(e2e::measure_inside(id))
                });
            overlay = overlay.child(cell);
        }
        overlay
    }

    fn activity_ranking(&self, data: &GranularityData, cx: &mut Context<Self>) -> impl IntoElement {
        let max = data.contributors.first().map_or(0, |c| c.commits);
        let mut rows = div()
            .id("activity-ranking-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.ui().activity_ranking_scroll)
            .flex()
            .flex_col();
        if data.contributors.is_empty() {
            rows = rows.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child("—"),
            );
        } else {
            rows = rows.children(
                data.contributors
                    .iter()
                    .take(50)
                    .enumerate()
                    .map(|(i, contributor)| contributor_row(i + 1, contributor, max)),
            );
        }
        div()
            .w(gpui::relative(0.4))
            .max_w(theme::scaled_px(300.))
            .flex_shrink_0()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme().accent_text_on(theme().panel)))
                    .flex_shrink_0()
                    .child("Top contributors"),
            )
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "activity-ranking-scrollbar",
                &self.ui().activity_ranking_scroll,
                rows,
                cx,
            ))
    }
}

fn period_label(gran: Granularity) -> &'static str {
    match gran {
        Granularity::Day => Msg::ActivityDay,
        Granularity::Week => Msg::ActivityWeek,
        Granularity::Month => Msg::ActivityMonth,
        Granularity::Year => Msg::ActivityYear,
        Granularity::All => Msg::ActivityAll,
    }
    .t()
}

fn bucket_keys(
    cell: gpui::Stateful<gpui::Div>,
    slot: usize,
    handles: std::rc::Rc<Vec<gpui::FocusHandle>>,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let go = |target: usize| {
        let handles = handles.clone();
        let entity = cx.weak_entity();
        move |window: &mut Window, cx: &mut gpui::App| {
            handles[target].focus(window, cx);
            let _ = entity.update(cx, |_, cx| cx.notify());
        }
    };
    let (prev, next, first, last) = (
        go(slot.saturating_sub(1)),
        go((slot + 1).min(handles.len() - 1)),
        go(0),
        go(handles.len() - 1),
    );
    cell.on_action(move |_: &keyboard_nav::TabListPrev, window, cx| prev(window, cx))
        .on_action(move |_: &keyboard_nav::TabListNext, window, cx| next(window, cx))
        .on_action(move |_: &keyboard_nav::TabListFirst, window, cx| first(window, cx))
        .on_action(move |_: &keyboard_nav::TabListLast, window, cx| last(window, cx))
}

fn period_cell(
    tabs: &keyboard_nav::TabList,
    slot: usize,
    id: &str,
    label: &'static str,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = SharedString::from(id.to_string());
    tabs.cell(slot, label, div().id(id.clone()))
        .relative()
        .flex()
        .items_center()
        .h(theme::scaled_px(22.))
        .px(keyboard_nav::inset(6.))
        .rounded(theme::scaled_px(4.))
        .text_xs()
        .cursor_default()
        .text_color(if selected {
            rgb(theme().text_main)
        } else {
            rgb(theme().text_muted)
        })
        .when(selected, |cell| cell.bg(theme().segment_selected_bg()))
        .when(!selected, |cell| cell.hover(|s| s.bg(rgb(theme().surface))))
        .child(label)
        .when(cfg!(feature = "gui-e2e"), |cell| {
            cell.child(e2e::measure_inside(id))
        })
}
