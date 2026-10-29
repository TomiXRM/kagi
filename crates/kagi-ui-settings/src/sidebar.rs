//! Sidebar composition and its fixed-hue category tiles.
use super::kit_style::{ink, metrics, pt, Hue};
use super::*;
use gpui_component::{Icon, IconName, Sizable, Size};
use kagi_ui_core::theme::{self, theme};
use kagi_ui_core::tooltip::HoverTooltip as _;

impl SettingsView {
    pub(crate) fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let ids: Vec<_> = Pane::ALL
            .iter()
            .map(|pane| format!("settings-pane-{}", pane.id()))
            .collect();
        if !ids.iter().any(|id| {
            self.focus
                .get(id)
                .is_some_and(|focus| focus.is_focused(window))
        }) {
            self.groups.insert(
                "sidebar".into(),
                Pane::ALL.iter().position(|p| *p == self.pane).unwrap_or(0),
            );
            // Focus left the sidebar: a later keyboard return shows its ring.
            self.sidebar_pointer_focus = None;
        }
        let focused = ids.iter().position(|id| {
            window.is_window_active()
                && self.sidebar_pointer_focus.as_ref() != Some(id)
                && self
                    .focus
                    .get(id)
                    .is_some_and(|focus| focus.is_focused(window))
        });
        self.layout_measurements.borrow_mut().sidebar_focus = None;
        let mut sidebar = div()
            .id("settings-sidebar-items")
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px(pt(8.) + px(1.))
            .pt(pt(40.) + px(1.))
            .pb(pt(8.))
            .flex()
            .flex_col()
            .gap(pt(2.))
            .role(Role::TabList);
        for (index, pane) in Pane::ALL.into_iter().enumerate() {
            let (icon, hue) = match pane {
                Pane::Appearance => (IconName::Palette, Hue::Indigo),
                Pane::Graph => (IconName::GalleryVerticalEnd, Hue::Orange),
                Pane::Editor => (IconName::File, Hue::Teal),
                Pane::Terminal => (IconName::SquareTerminal, Hue::Gray),
                Pane::Network => (IconName::Network, Hue::Blue),
                Pane::SmartCommit => (IconName::Bot, Hue::Purple),
                Pane::Analyze => (IconName::ChartPie, Hue::Teal),
                Pane::General => (IconName::Settings2, Hue::Gray),
            };
            let selected = pane == self.pane;
            let hue = hue.rgb(theme().dark);
            let item = self
                .control(
                    ids[index].clone(),
                    pane.title().into(),
                    selected,
                    true,
                    Role::Tab,
                    ink::sidebar(),
                    px(0.),
                    10.,
                    Some(SettingsIntent::Pane(pane)),
                    Some(("sidebar".into(), index, ids.clone())),
                    window,
                    cx,
                )
                .flex()
                .items_center()
                .min_h(pt(metrics::SIDEBAR_ROW))
                .gap(pt(9.))
                .px(pt(8.))
                .py(pt(4.))
                .rounded(pt(8.))
                .when(selected, |el| el.bg(ink::selected()))
                .when(!selected, |el| {
                    el.hover(|s| s.bg(rgb(ink::control_hover())))
                        .active(|s| s.bg(rgb(ink::control_pressed())))
                })
                .child(
                    div()
                        .relative()
                        .size(pt(metrics::TILE))
                        .flex_shrink_0()
                        .rounded(pt(5.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(rgb(super::kit_style::mix(hue, 0xffffff, 0.22)), 0.),
                            linear_color_stop(rgb(hue), 1.),
                        ))
                        .text_color(rgb(theme::on_accent_foreground(hue)))
                        .when(cfg!(feature = "gui-e2e"), |tile| {
                            tile.child(self.measured(
                                format!("settings-pane-tile-{}", pane.id()),
                                Role::Image,
                                pane.title().into(),
                                String::new(),
                                false,
                                false,
                                false,
                            ))
                        })
                        .child(Icon::new(icon).with_size(Size::Size(pt(12.4)))),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .truncate()
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .when(cfg!(feature = "gui-e2e"), |label| {
                            label.child(self.measured(
                                format!("{}-label", ids[index]),
                                Role::Label,
                                pane.title().into(),
                                String::new(),
                                false,
                                selected,
                                false,
                            ))
                        })
                        .child(pane.title()),
                );
            let item = item.when(focused == Some(index), |item| {
                let measurements = self.layout_measurements.clone();
                item.child(
                    canvas(
                        move |bounds, _, _| {
                            measurements.borrow_mut().sidebar_focus = Some(bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            });
            sidebar = sidebar.child(item.hover_tooltip(
                SharedString::from(format!("settings-pane-name-{}", pane.id())),
                pane.title(),
            ));
        }
        if let Some(index) = focused {
            sidebar = sidebar.child(self.sidebar_focus_overlay(index));
        }
        let sidebar = sidebar
            .relative()
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-sidebar-items".into(),
                    Role::TabList,
                    Msg::SettingsTitle.t().into(),
                    String::new(),
                    false,
                    false,
                    false,
                )
            }));
        div()
            .relative()
            .h_full()
            .w(pt(metrics::SIDEBAR_WIDTH) + px(metrics::SIDEBAR_INSET * 2.))
            .max_w(relative(0.55))
            .min_w_0()
            .flex_shrink(1.)
            .p(px(metrics::SIDEBAR_INSET))
            .bg(rgb(ink::window()))
            .child(
                div()
                    .id("settings-sidebar-panel")
                    .absolute()
                    .inset(px(metrics::SIDEBAR_INSET))
                    .rounded(px(metrics::SIDEBAR_RADIUS))
                    .border_1()
                    .border_color(rgb(ink::hairline()))
                    .bg(rgb(ink::sidebar()))
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            "settings-sidebar-panel".into(),
                            Role::Group,
                            Msg::SettingsTitle.t().into(),
                            String::new(),
                            false,
                            false,
                            false,
                        )
                    })),
            )
            .child(
                div()
                    .id("settings-sidebar-scrollbar")
                    .relative()
                    .size_full()
                    .min_h_0()
                    .child(
                        div()
                            .id("settings-sidebar")
                            .size_full()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll)
                            .map(|mut viewport| {
                                viewport.style().restrict_scroll_to_axis = Some(true);
                                viewport
                            })
                            .child(sidebar),
                    )
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            "settings-sidebar".into(),
                            Role::TabList,
                            Msg::SettingsTitle.t().into(),
                            String::new(),
                            false,
                            false,
                            false,
                        )
                    }))
                    .child(
                        gpui_component::scroll::Scrollbar::vertical(&self.sidebar_scroll)
                            .id("settings-sidebar-scrollbar"),
                    ),
            )
            .into_any_element()
    }

    fn sidebar_focus_overlay(&self, index: usize) -> AnyElement {
        let measurements = self.layout_measurements.clone();
        let background = ink::sidebar();
        let color = super::focus_ring::color(self.snapshot.effective_accent, background);
        #[cfg(feature = "gui-e2e")]
        let (id, probes) = (
            format!("settings-pane-{}-focus-ring", Pane::ALL[index].id()),
            self.probes.clone(),
        );
        #[cfg(not(feature = "gui-e2e"))]
        let _ = index;
        // Paint after rows, but before later host content/overlays. A global
        // deferred draw would incorrectly lift decoration above host overlays.
        canvas(
            move |_, _, _| measurements.borrow().sidebar_focus,
            move |_, bounds, window, _| {
                let Some(bounds) = bounds else { return };
                let bounds = bounds.dilate(px(5.));
                window.paint_quad(quad(
                    bounds,
                    pt(10.),
                    transparent_black(),
                    px(5.),
                    rgb(background),
                    BorderStyle::Solid,
                ));
                window.paint_quad(quad(
                    bounds,
                    pt(10.),
                    transparent_black(),
                    px(2.),
                    color,
                    BorderStyle::Solid,
                ));
                #[cfg(feature = "gui-e2e")]
                probes.borrow_mut().insert(
                    id.clone(),
                    ControlProbe {
                        id,
                        bounds,
                        role: Role::Image,
                        name: String::new(),
                        value: String::new(),
                        paint: None,
                        focused: false,
                        selected: false,
                        disabled: false,
                    },
                );
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
}
