//! Shared interaction, grouped rows and navigation for the final settings kit.
use super::kit_style::{ink, metrics, pt};
use super::*;
use gpui_component::{ElementExt as _, Icon, IconName, Sizable, Size};
use kagi_ui_core::theme::{self, theme};
use kagi_ui_core::tooltip::HoverTooltip as _;

#[derive(IntoElement)]
pub(crate) struct SettingsGroup {
    container: Stateful<Div>,
    rows: usize,
    #[cfg(feature = "gui-e2e")]
    id: &'static str,
    #[cfg(feature = "gui-e2e")]
    probes: Rc<RefCell<BTreeMap<String, ControlProbe>>>,
}

impl ParentElement for SettingsGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        for element in elements {
            let element = if self.rows > 0 {
                let separator = div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(rgb(ink::hairline()));
                #[cfg(feature = "gui-e2e")]
                let separator = {
                    let id = format!("{}-separator-{}", self.id, self.rows - 1);
                    let probes = self.probes.clone();
                    separator.on_prepaint(move |bounds, _, _| {
                        probes.borrow_mut().insert(
                            id.clone(),
                            ControlProbe {
                                id: id.clone(),
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
                    })
                };
                div()
                    .relative()
                    .min_w_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(element)
                    .child(separator)
                    .into_any_element()
            } else {
                element
            };
            self.container.extend([element]);
            self.rows += 1;
        }
    }
}

impl RenderOnce for SettingsGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.container
    }
}

pub(crate) fn section(
    title: Msg,
    subtitle: Option<AnyElement>,
    content: impl IntoElement,
) -> AnyElement {
    div()
        .id(SharedString::from(format!(
            "settings-section-{}",
            title.t()
        )))
        .role(Role::Group)
        .aria_label(title.t())
        .min_w_0()
        .flex()
        .flex_col()
        .gap(pt(8.))
        .child(
            div()
                .px(pt(metrics::GROUP_PADDING_X) + px(1.))
                .min_w_0()
                .flex()
                .flex_col()
                .gap(pt(2.))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "settings-heading-{}",
                            title.t()
                        )))
                        .role(Role::Heading)
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .font_weight(FontWeight::BOLD)
                        .child(title.t()),
                )
                .children(subtitle.map(|subtitle| {
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .text_size(pt(metrics::SECONDARY + 0.5))
                        .text_color(ink::secondary())
                        .child(subtitle)
                })),
        )
        .child(content)
        .into_any_element()
}
impl SettingsView {
    pub(crate) fn group(&self, id: &'static str) -> SettingsGroup {
        let container = div()
            .id(id)
            .relative()
            .min_w_0()
            .w_full()
            .rounded(pt(metrics::GROUP_RADIUS))
            .px(pt(metrics::GROUP_PADDING_X))
            .border_1()
            .border_color(if theme().dark {
                transparent_black()
            } else {
                rgb(ink::hairline()).into()
            })
            .bg(rgb(ink::card()))
            .flex()
            .flex_col();
        #[cfg(feature = "gui-e2e")]
        let container = {
            let probes = self.probes.clone();
            container.child(
                canvas(
                    move |bounds, _, _| {
                        // Explicit top/left gives the padding box, unlike the
                        // static-position canvas in ElementExt::on_prepaint.
                        probes.borrow_mut().insert(
                            id.into(),
                            ControlProbe {
                                id: id.into(),
                                bounds: bounds.dilate(px(1.)),
                                role: Role::Group,
                                name: id.into(),
                                value: String::new(),
                                paint: None,
                                focused: false,
                                selected: false,
                                disabled: false,
                            },
                        );
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
        };
        SettingsGroup {
            container,
            rows: 0,
            #[cfg(feature = "gui-e2e")]
            id,
            #[cfg(feature = "gui-e2e")]
            probes: self.probes.clone(),
        }
    }
    pub(crate) fn row(
        &self,
        title: Msg,
        description: impl Into<SharedString>,
        control: impl IntoElement,
    ) -> AnyElement {
        let description = description.into();
        let summary = match title {
            Msg::SettingsLaneCompact => Some(Msg::SettingsLaneCompactSummary),
            Msg::SettingsAutoFetch => Some(Msg::SettingsAutoFetchSummary),
            Msg::SettingsGraphCopy => Some(Msg::SettingsGraphCopySummary),
            Msg::SettingsReduceMotion => Some(Msg::SettingsReduceMotionSummary),
            Msg::SettingsTerminalAutoLock => Some(Msg::SettingsTerminalAutoLockSummary),
            Msg::SettingsSmartEnable => Some(Msg::SettingsSmartEnableSummary),
            Msg::SettingsSmartProvider => Some(Msg::SettingsSmartProviderSummary),
            _ => None,
        };
        let visible: SharedString =
            summary.map_or_else(|| description.clone(), |text| text.t().into());
        let id = match title {
            Msg::SettingsLaneCompact => "settings-avatar-help",
            Msg::SettingsAutoFetch => "settings-fetch-help",
            Msg::SettingsGraphCopy => "settings-copy-help",
            Msg::SettingsReduceMotion => "settings-motion-help",
            Msg::SettingsTerminalAutoLock => "settings-terminal-lock-help",
            Msg::SettingsSmartEnable => "settings-smart-enable-help",
            Msg::SettingsSmartProvider => "settings-provider-help",
            Msg::SettingsThemeFolder => "settings-theme-folder-help",
            Msg::SettingsAccent => "settings-accent-help",
            Msg::SettingsToolbarDisplay => "settings-toolbar-help",
            Msg::SettingsZoom => "settings-zoom-help",
            Msg::SettingsCompactThreshold => "settings-threshold-help",
            Msg::SettingsCompact => "settings-compact-help",
            Msg::SettingsMarkdownDefaultView => "settings-markdown-help",
            Msg::SettingsInterfaceLang => "settings-language-help",
            Msg::SettingsSmartModel => "settings-model-help",
            _ => unreachable!("only Settings row titles are accepted"),
        };
        let row = div()
            .id(SharedString::from(format!(
                "{}-row",
                id.trim_end_matches("-help")
            )))
            .role(Role::Group)
            .aria_label(title.t())
            .aria_description(description.clone())
            .relative()
            .min_w_0()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .min_h(pt(if description.is_empty() {
                metrics::ROW
            } else {
                metrics::ROW_WITH_SUBTITLE
            }))
            .gap(pt(12.))
            .py(pt(7.))
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    format!("{}-row", id.trim_end_matches("-help")),
                    Role::Group,
                    title.t().into(),
                    String::new(),
                    false,
                    false,
                    false,
                )
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(pt(140.))
                    .flex()
                    .flex_col()
                    .gap(pt(2.))
                    .child(title.t())
                    .children((!visible.is_empty()).then(|| {
                        let text = StyledText::new(visible.clone());
                        #[cfg(feature = "gui-e2e")]
                        let text_probe = Some(self.measured_text(
                            format!("{id}-text"),
                            visible.to_string(),
                            &text,
                        ));
                        #[cfg(not(feature = "gui-e2e"))]
                        let text_probe = None::<AnyElement>;
                        div()
                            .id(id)
                            .relative()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .role(Role::Label)
                            .aria_label(visible.clone())
                            .aria_description(description.clone())
                            .text_size(pt(metrics::SECONDARY))
                            .text_color(ink::secondary())
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    id.into(),
                                    Role::Label,
                                    title.t().into(),
                                    visible.to_string(),
                                    false,
                                    false,
                                    false,
                                )
                            }))
                            .child(text)
                            .children(text_probe)
                    })),
            )
            // Wrap the whole composite below its label, not its internal controls.
            .child(
                div()
                    .flex_shrink_0()
                    .min_w_0()
                    .max_w_full()
                    .ml_auto()
                    .child(control),
            );
        if summary.is_some() {
            row.hover_tooltip(SharedString::from(format!("{id}-full")), description)
                .into_any_element()
        } else {
            row.into_any_element()
        }
    }
    pub(super) fn handle(&mut self, id: &str, cx: &mut App) -> FocusHandle {
        self.focus
            .entry(id.to_owned())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }
    // Keep the recorder's accessibility and interaction-state inputs explicit.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn measured(
        &self,
        id: String,
        role: Role,
        name: String,
        value: String,
        focused: bool,
        selected: bool,
        disabled: bool,
    ) -> AnyElement {
        #[cfg(feature = "gui-e2e")]
        {
            let probes = self.probes.clone();
            let paint_probes = probes.clone();
            let paint_id = id.clone();
            let focus = self.focus.get(&id).cloned();
            canvas(
                move |bounds, window, _| {
                    let focused = focus
                        .as_ref()
                        .map_or(focused, |focus| focus.is_focused(window));
                    probes.borrow_mut().insert(
                        id.clone(),
                        ControlProbe {
                            id,
                            bounds,
                            focused,
                            selected,
                            disabled,
                            role,
                            name,
                            value,
                            paint: None,
                        },
                    );
                },
                move |_, _, window, _| {
                    if let Some(probe) = paint_probes.borrow_mut().get_mut(&paint_id) {
                        let style = window.text_style();
                        probe.paint = Some(ControlPaint {
                            font_size: style.font_size.to_pixels(window.rem_size()),
                            font_weight: style.font_weight,
                            text_color: style.color,
                        });
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element()
        }
        #[cfg(not(feature = "gui-e2e"))]
        {
            let _ = (id, role, name, value, focused, selected, disabled);
            div().absolute().into_any_element()
        }
    }
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn measured_text(&self, id: String, name: String, text: &StyledText) -> AnyElement {
        let layout = text.layout().clone();
        let probes = self.probes.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let style = window.text_style();
                probes.borrow_mut().insert(
                    id.clone(),
                    ControlProbe {
                        id,
                        bounds: layout.bounds(),
                        focused: false,
                        selected: false,
                        disabled: false,
                        role: Role::Label,
                        name,
                        value: layout.text(),
                        paint: Some(ControlPaint {
                            font_size: style.font_size.to_pixels(window.rem_size()),
                            font_weight: style.font_weight,
                            text_color: style.color,
                        }),
                    },
                );
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size(px(0.))
        .into_any_element()
    }
    // One leaf builder owns accessibility, roving focus and the typed intent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn control(
        &mut self,
        id: String,
        label: SharedString,
        selected: bool,
        enabled: bool,
        role: Role,
        focus_background: u32,
        border_inset: Pixels,
        focus_radius: f32,
        intent: Option<SettingsIntent>,
        roving: Option<(String, usize, Vec<String>)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let handle = self.handle(&id, cx);
        let focused = handle.is_focused(window) && window.is_window_active();
        let tab_stop = roving
            .as_ref()
            .is_none_or(|(group, slot, _)| *self.groups.get(group).unwrap_or(&0) == *slot);
        let click_intent = intent.clone();
        let focus = handle.clone();
        let key_focus = handle.clone();
        let reveal_focus = handle.clone();
        // Theme sections own coordinated horizontal/vertical reveal, including
        // their headings. Do not also scroll just the card's leaf bounds.
        let revealed = roving
            .as_ref()
            .is_none_or(|(group, _, _)| group != "themes")
            .then(|| self.revealed.entry(id.clone()).or_default().clone());
        let scroll = if id.starts_with("settings-pane-") {
            self.sidebar_scroll.clone()
        } else {
            self.scroll.clone()
        };
        let reveal_selected = role == Role::Tab
            && selected
            && roving.as_ref().is_some_and(|(_, _, ids)| {
                ids.iter().all(|id| {
                    self.focus
                        .get(id)
                        .is_none_or(|focus| !focus.is_focused(window))
                })
            });
        let focus_overlay = focused.then(|| {
            let ring_color =
                super::focus_ring::color(self.snapshot.effective_accent, focus_background);
            div()
                .absolute()
                .inset(-(border_inset + px(5.)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(pt(focus_radius))
                        .border(px(5.))
                        .border_color(rgb(focus_background)),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(pt(focus_radius))
                        .border_2()
                        .border_color(ring_color),
                )
                .when(cfg!(feature = "gui-e2e"), |ring| {
                    ring.child(self.measured(
                        format!("{id}-focus-ring"),
                        Role::Image,
                        String::new(),
                        String::new(),
                        false,
                        false,
                        false,
                    ))
                })
        });
        let probe = cfg!(feature = "gui-e2e").then(|| {
            self.measured(
                id.clone(),
                role,
                label.to_string(),
                label.to_string(),
                focused,
                selected,
                !enabled,
            )
        });
        div()
            .group(SharedString::from(id.clone()))
            .id(SharedString::from(id))
            .relative()
            .min_h(pt(metrics::CONTROL_HIT_HEIGHT))
            .min_w(pt(metrics::CONTROL_HIT_HEIGHT))
            .rounded(pt(metrics::CONTROL_RADIUS))
            .track_focus(&handle.tab_index(0).tab_stop(tab_stop && enabled))
            .role(role)
            .aria_label(label)
            .aria_selected(selected)
            .when(role == Role::Switch, |el| {
                el.aria_toggled(if selected {
                    Toggled::True
                } else {
                    Toggled::False
                })
            })
            .when(!enabled, |el| {
                el.a11y_synthetic_children(|builder: &mut A11ySubtreeBuilder| {
                    builder.parent_node().set_disabled()
                })
            })
            .when(enabled, |el| el.cursor_pointer())
            .children(focus_overlay)
            .children(probe)
            .on_prepaint(move |bounds, window, cx| {
                let Some(revealed) = &revealed else {
                    return;
                };
                let focused = reveal_focus.is_focused(window);
                if !focused && !reveal_selected {
                    revealed.set(None);
                    return;
                }
                let viewport = scroll.bounds();
                let marker = ControlReveal {
                    viewport: viewport.size,
                    focused,
                };
                if revealed.get() == Some(marker) {
                    return;
                }
                let margin = px(metrics::FOCUS_REVEAL_MARGIN);
                let delta = if bounds.size.height > viewport.size.height - margin * 2.
                    || bounds.top() < viewport.top() + margin
                {
                    viewport.top() + margin - bounds.top()
                } else if bounds.bottom() > viewport.bottom() - margin {
                    viewport.bottom() - margin - bounds.bottom()
                } else {
                    revealed.set(Some(marker));
                    return;
                };
                let old = scroll.offset();
                let offset = point(old.x, (old.y + delta).clamp(-scroll.max_offset().y, px(0.)));
                if old != offset {
                    let scroll = scroll.clone();
                    window.defer(cx, move |window, _| {
                        scroll.set_offset(offset);
                        window.refresh();
                    });
                } else {
                    revealed.set(Some(marker));
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    window.focus(&focus, cx);
                    if let Some(intent) = click_intent.clone() {
                        this.emit(intent, window, cx);
                    }
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !key_focus.is_focused(window) {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "space" | "enter" => {
                        cx.stop_propagation();
                    }
                    "left" | "right" | "up" | "down" | "home" | "end" => {
                        if let Some((group, index, ids)) = &roving {
                            if ids.is_empty() {
                                return;
                            }
                            let next = match event.keystroke.key.as_str() {
                                "home" => 0,
                                "end" => ids.len() - 1,
                                "left" | "up" => (index + ids.len() - 1) % ids.len(),
                                _ => (index + 1) % ids.len(),
                            };
                            this.groups.insert(group.clone(), next);
                            let focus = this.handle(&ids[next], cx);
                            window.focus(&focus, cx);
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
    }
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
    pub(crate) fn repair_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pane == Pane::Appearance {
            let removed_focused_theme = self.theme_reveal.as_ref().is_some_and(|last| {
                last.previous_catalog.as_ref().is_some_and(|previous| {
                    self.focus.iter().any(|(id, focus)| {
                        focus.is_focused(window)
                            && id.strip_prefix("settings-theme-").is_some_and(|slug| {
                                previous.iter().any(|theme| theme.slug == slug)
                                    && !self.snapshot.themes.iter().any(|theme| theme.slug == slug)
                            })
                    })
                })
            });
            if removed_focused_theme {
                let id = self
                    .snapshot
                    .themes
                    .iter()
                    .find(|theme| theme.slug == self.snapshot.theme_slug.as_ref())
                    .map_or_else(
                        || "settings-pane-appearance".to_owned(),
                        |theme| format!("settings-theme-{}", theme.slug),
                    );
                self.handle(&id, cx).focus(window, cx);
            }
            let zoom = self.snapshot.zoom;
            let threshold = self.snapshot.compact_threshold_rem;
            for (id, opposite, disabled) in [
                (
                    "zoom-dec",
                    "zoom-inc",
                    theme::step_zoom(zoom, false) == zoom,
                ),
                ("zoom-inc", "zoom-dec", theme::step_zoom(zoom, true) == zoom),
                (
                    "compact-threshold-dec",
                    "compact-threshold-inc",
                    threshold <= 40,
                ),
                (
                    "compact-threshold-inc",
                    "compact-threshold-dec",
                    threshold >= 160,
                ),
            ] {
                if disabled
                    && self
                        .focus
                        .get(id)
                        .is_some_and(|focus| focus.is_focused(window))
                {
                    let focus = self.handle(opposite, cx);
                    window.focus(&focus, cx);
                }
            }
        }
        let model_focused = self
            .focus
            .get("sc-model-select")
            .is_some_and(|focus| focus.is_focused(window));
        if self.pane != Pane::SmartCommit
            || self.snapshot.smart_provider != SmartProvider::Ollama
            || !self.snapshot.smart_ollama_available
            || self.snapshot.smart_detecting
            || self.snapshot.models.is_empty()
        {
            if model_focused {
                let id = if self.pane == Pane::SmartCommit {
                    "sc-provider-select".to_owned()
                } else {
                    format!("settings-pane-{}", self.pane.id())
                };
                let focus = self.handle(&id, cx);
                window.focus(&focus, cx);
            }
        }
    }
}
