//! Sidebar composition and its fixed-hue category tiles.
use super::kit_style::{ink, metrics, pt};
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
        }
        let mut sidebar = div()
            .id("settings-sidebar-items")
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .p(pt(8.))
            .flex()
            .flex_col()
            .gap(pt(2.).max(px(6.)))
            .role(Role::TabList);
        for (index, pane) in Pane::ALL.into_iter().enumerate() {
            let (icon, light, dark) = match pane {
                Pane::Appearance => (IconName::Palette, 0x5856d6, 0x5e5ce6),
                Pane::Graph => (IconName::Network, 0xff9500, 0xff9f0a),
                Pane::Editor => (IconName::File, 0x30b0c7, 0x40c8e0),
                Pane::Terminal => (IconName::SquareTerminal, 0x8e8e93, 0x6e6e73),
                Pane::Network => (IconName::Globe, 0x007aff, 0x0a84ff),
                Pane::SmartCommit => (IconName::Bot, 0xaf52de, 0xbf5af2),
                Pane::Analyze => (IconName::ChartPie, 0x30b0c7, 0x40c8e0),
                Pane::General => (IconName::Settings2, 0x8e8e93, 0x6e6e73),
            };
            let selected = pane == self.pane;
            let hue = if theme().dark { dark } else { light };
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
                .px(pt(6.))
                .py(pt(2.))
                .when(selected, |el| el.bg(rgb(theme().selected)))
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
            sidebar = sidebar.child(item.hover_tooltip(
                SharedString::from(format!("settings-pane-name-{}", pane.id())),
                pane.title(),
            ));
        }
        let chrome = self.chrome_heights.clone();
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
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        chrome.borrow_mut().1 = bounds.size.height;
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        div()
            .relative()
            .h_full()
            .w(pt(metrics::SIDEBAR_WIDTH + 16.))
            .max_w(relative(0.55))
            .min_w_0()
            .flex_shrink(1.)
            .p(pt(8.))
            .bg(rgb(ink::window()))
            .child(
                div()
                    .absolute()
                    .inset(pt(8.))
                    .rounded(pt(12.))
                    .border_1()
                    .border_color(rgb(ink::hairline()))
                    .bg(rgb(ink::sidebar())),
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
}
