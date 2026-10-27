//! Production adaptation of the approved settings_kit: grouped rows, equal
//! segments and fixed-hue tiles. Real focus replaces the experiment's Forced state.
use super::*;
use gpui_component::{ElementExt as _, Icon, IconName, Sizable, Size};
use kagi_ui_core::theme::{self, theme};

pub(crate) fn pt(n: f32) -> Pixels {
    theme::scaled_px(n * 16. / theme::BASE_REM_PX)
}

fn focus_ring_color(accent: u32, background: u32) -> Hsla {
    let base: Hsla = rgb(accent).into();
    let surface: Hsla = rgb(background).into();
    let candidate = |lightness| Hsla {
        l: lightness,
        a: 0.75,
        ..base
    };
    let contrast =
        |ring| theme::contrast_ratio(u32::from(surface.blend(ring).to_rgb()) >> 8, background);
    let initial = candidate(base.l);
    // Leave rounding headroom for the final composited native/browser pixels.
    if contrast(initial) >= 3.1 {
        return initial;
    }
    let endpoint = if contrast(candidate(0.)) >= contrast(candidate(1.)) {
        0.
    } else {
        1.
    };
    for step in 1..=32 {
        let ring = candidate(base.l + (endpoint - base.l) * step as f32 / 32.);
        if contrast(ring) >= 3.1 {
            return ring;
        }
    }
    candidate(endpoint)
}

#[cfg(test)]
#[test]
fn focus_ring_is_translucent_and_contrasts_after_compositing() {
    for background in [0xf7f7f7, 0xffffff, 0xe5e5e5, 0x1e1e1e, 0x29292c, 0x444444] {
        for accent in [0xffcc00, 0x007aff, 0x0a84ff, 0xaf52de, 0xffffff, 0x000000] {
            let ring = focus_ring_color(accent, background);
            let surface: Hsla = rgb(background).into();
            let base: Hsla = rgb(accent).into();
            assert_eq!(ring.a, 0.75);
            assert_eq!(ring.h, base.h);
            assert_eq!(ring.s, base.s);
            let painted = u32::from(surface.blend(ring).to_rgb()) >> 8;
            assert!(theme::contrast_ratio(painted, background) >= 3.0);
        }
    }
}

pub(crate) fn section(title: Msg, content: impl IntoElement) -> AnyElement {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(pt(8.))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.t()))
        .child(content)
        .into_any_element()
}
impl SettingsView {
    pub(crate) fn group(&self, id: &'static str) -> Stateful<Div> {
        div()
            .id(id)
            .relative()
            .min_w_0()
            .w_full()
            .rounded(pt(10.))
            .bg(rgb(theme().bg_base))
            .flex()
            .flex_col()
            .child(self.measured(
                id.into(),
                Role::Group,
                id.into(),
                String::new(),
                false,
                false,
                false,
            ))
    }
    pub(crate) fn row(
        &self,
        title: Msg,
        description: impl Into<SharedString>,
        control: impl IntoElement,
    ) -> AnyElement {
        let description = description.into();
        let id = match title {
            Msg::SettingsLaneCompact => "settings-avatar-help",
            Msg::SettingsAutoFetch => "settings-fetch-help",
            Msg::SettingsGraphCopy => "settings-copy-help",
            Msg::SettingsReduceMotion => "settings-motion-help",
            Msg::SettingsTerminalAutoLock => "settings-terminal-lock-help",
            Msg::SettingsSmartEnable => "settings-smart-enable-help",
            Msg::SettingsSmartProvider => "settings-provider-help",
            Msg::SettingsCustomThemes => "settings-theme-folder-help",
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
        div()
            .min_w_0()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(pt(10.))
            .px(pt(12.))
            .py(pt(10.))
            .child(
                div()
                    .flex_1()
                    .min_w(pt(140.))
                    .flex()
                    .flex_col()
                    .gap(pt(4.))
                    .child(title.t())
                    .child(
                        div()
                            .id(id)
                            .relative()
                            .text_size(pt(11.5))
                            .text_color(rgb(theme().text_sub))
                            .child(description.clone())
                            .child(self.measured(
                                id.into(),
                                Role::Label,
                                title.t().into(),
                                description.to_string(),
                                false,
                                false,
                                false,
                            )),
                    ),
            )
            .child(div().min_w_0().max_w_full().child(control))
            .into_any_element()
    }
    fn handle(&mut self, id: &str, cx: &mut App) -> FocusHandle {
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
        intent: SettingsIntent,
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
        let revealed = self.revealed.entry(id.clone()).or_default().clone();
        let scroll = self.scroll.clone();
        let in_detail = !id.starts_with("settings-pane-");
        let ring_color = if focused {
            focus_ring_color(self.snapshot.effective_accent, focus_background)
        } else {
            transparent_black()
        };
        let probe = self.measured(
            id.clone(),
            role,
            label.to_string(),
            label.to_string(),
            focused,
            selected,
            !enabled,
        );
        div()
            .id(SharedString::from(id))
            .relative()
            .min_h(pt(28.))
            .min_w(pt(28.))
            .rounded(pt(7.))
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
                el.opacity(0.5)
                    .a11y_synthetic_children(|builder: &mut A11ySubtreeBuilder| {
                        builder.parent_node().set_disabled()
                    })
            })
            .when(enabled, |el| el.cursor_pointer())
            .when(enabled && !selected && role != Role::RadioButton, |el| {
                el.hover(|s| s.bg(rgb(theme().surface)))
            })
            .child(
                div()
                    .absolute()
                    .inset(px(-5.))
                    .rounded(pt(10.))
                    .border_2()
                    .border_color(if focused {
                        ring_color
                    } else {
                        transparent_black()
                    }),
            )
            .child(probe)
            .on_prepaint(move |bounds, window, cx| {
                if !reveal_focus.is_focused(window) {
                    revealed.set(false);
                    return;
                }
                if !in_detail || revealed.get() {
                    return;
                }
                let viewport = scroll.bounds();
                let margin = px(7.);
                let delta = if bounds.size.height > viewport.size.height - margin * 2.
                    || bounds.top() < viewport.top() + margin
                {
                    viewport.top() + margin - bounds.top()
                } else if bounds.bottom() > viewport.bottom() - margin {
                    viewport.bottom() - margin - bounds.bottom()
                } else {
                    revealed.set(true);
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
                    revealed.set(true);
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    window.focus(&focus, cx);
                    this.emit(click_intent.clone(), window, cx);
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn button(
        &mut self,
        id: &str,
        label: SharedString,
        enabled: bool,
        focus_background: u32,
        intent: SettingsIntent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.control(
            id.into(),
            label.clone(),
            false,
            enabled,
            Role::Button,
            focus_background,
            intent,
            None,
            window,
            cx,
        )
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .whitespace_nowrap()
        .px(pt(10.))
        .py(pt(3.))
        .bg(rgb(theme().surface))
        .child(label)
        .into_any_element()
    }
    pub(crate) fn stepper_button(
        &mut self,
        id: &str,
        label: SharedString,
        enabled: bool,
        intent: SettingsIntent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.control(
            id.into(),
            label,
            false,
            enabled,
            Role::Button,
            theme().bg_base,
            intent,
            None,
            window,
            cx,
        )
        .size(pt(28.))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(theme().surface))
        .child(if id.ends_with("-inc") { "+" } else { "−" })
        .into_any_element()
    }
    pub(crate) fn toggle(
        &mut self,
        id: &str,
        title: Msg,
        checked: bool,
        intent: SettingsIntent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.control(
            id.into(),
            title.t().into(),
            checked,
            true,
            Role::Switch,
            theme().bg_base,
            intent,
            None,
            window,
            cx,
        )
        .w(pt(40.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .relative()
                .w(pt(32.))
                .h(pt(18.))
                .rounded_full()
                .bg(rgb(if checked {
                    self.snapshot.effective_accent
                } else {
                    theme().selected
                }))
                .child(
                    div()
                        .absolute()
                        .top(pt(2.))
                        .left(pt(if checked { 16. } else { 2. }))
                        .size(pt(14.))
                        .rounded_full()
                        .bg(white())
                        .shadow_sm(),
                ),
        )
        .into_any_element()
    }
    pub(crate) fn choices(
        &mut self,
        group: &str,
        entries: Vec<(String, SharedString, bool, bool, SettingsIntent)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ids: Vec<_> = entries.iter().map(|entry| entry.0.clone()).collect();
        let selected = entries.iter().position(|entry| entry.2).unwrap_or(0);
        if !ids.iter().any(|id| {
            self.focus
                .get(id)
                .is_some_and(|focus| focus.is_focused(window))
        }) {
            let entry = entries
                .iter()
                .position(|entry| entry.2 && entry.3)
                .or_else(|| entries.iter().position(|entry| entry.3))
                .unwrap_or(selected);
            self.groups.insert(group.into(), entry);
        }
        let mut track = div()
            .id(SharedString::from(group.to_owned()))
            .relative()
            .min_w_0()
            .w(pt(144. * entries.len() as f32))
            .max_w_full()
            .flex()
            .flex_wrap()
            .gap(pt(4.))
            .px(pt(2.))
            .role(Role::RadioGroup)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(pt(2.))
                    .bottom(pt(2.))
                    .rounded(pt(6.))
                    .bg(rgb(theme().surface))
                    .when(cfg!(feature = "gui-e2e"), |visual| {
                        visual.child(self.measured(
                            format!("{group}-visual"),
                            Role::Image,
                            String::new(),
                            String::new(),
                            false,
                            false,
                            false,
                        ))
                    }),
            );
        for (index, (id, label, selected, enabled, intent)) in entries.into_iter().enumerate() {
            let control = self
                .control(
                    id,
                    label.clone(),
                    selected,
                    enabled,
                    Role::RadioButton,
                    theme().bg_base,
                    intent,
                    Some((group.into(), index, ids.clone())),
                    window,
                    cx,
                )
                .flex_1()
                .min_w(pt(64.))
                .px(pt(8.))
                .py(pt(4.))
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .when(selected, |el| {
                    // The Settings segment is raised, not an inactive row (#1816).
                    let fill = match theme().slug.as_ref() {
                        "apple-light" => white(),
                        "apple-dark" => rgb(0x66666c).into(),
                        _ => theme().segment_selected_bg(),
                    };
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(pt(4.))
                            .bottom(pt(4.))
                            .rounded(pt(4.))
                            .bg(fill)
                            .shadow_sm()
                            .when(cfg!(feature = "gui-e2e"), |visual| {
                                visual.child(self.measured(
                                    format!("{}-visual", ids[index]),
                                    Role::Image,
                                    String::new(),
                                    String::new(),
                                    false,
                                    false,
                                    false,
                                ))
                            }),
                    )
                })
                .child(div().min_w_0().child(label));
            track = track.child(control);
        }
        track.into_any_element()
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
            .id("settings-sidebar")
            .w(pt(176.))
            .flex_shrink_0()
            .p(pt(8.))
            .flex()
            .flex_col()
            .gap(pt(4.))
            .bg(rgb(theme().surface))
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
            let item = self
                .control(
                    ids[index].clone(),
                    pane.title().into(),
                    selected,
                    true,
                    Role::Tab,
                    theme().surface,
                    SettingsIntent::Pane(pane),
                    Some(("sidebar".into(), index, ids.clone())),
                    window,
                    cx,
                )
                .flex()
                .items_center()
                .gap(pt(8.))
                .px(pt(6.))
                .py(pt(5.))
                .when(selected, |el| el.bg(rgb(theme().selected)))
                .child(
                    div()
                        .relative()
                        .size(pt(20.))
                        .flex_shrink_0()
                        .rounded(pt(5.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(if theme().dark { dark } else { light }))
                        .text_color(white())
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
                .child(div().min_w_0().child(pane.title()));
            sidebar = sidebar.child(item);
        }
        let chrome = self.chrome_heights.clone();
        let sidebar = sidebar
            .relative()
            .child(self.measured(
                "settings-sidebar".into(),
                Role::TabList,
                Msg::SettingsTitle.t().into(),
                String::new(),
                false,
                false,
                false,
            ))
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
            .h_full()
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .child(sidebar)
            .into_any_element()
    }
    pub(crate) fn repair_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pane == Pane::Appearance {
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
        let focused_model_missing = self
            .focus
            .iter()
            .find(|(_, focus)| focus.is_focused(window))
            .and_then(|(id, _)| id.strip_prefix("sc-model-"))
            .is_some_and(|model| {
                !self
                    .snapshot
                    .models
                    .iter()
                    .any(|candidate| candidate.as_ref() == model)
            });
        if self.pane != Pane::SmartCommit
            || self.snapshot.smart_provider != SmartProvider::Ollama
            || !self.snapshot.smart_ollama_available
            || self.snapshot.smart_detecting
            || focused_model_missing
        {
            let model_focused = self
                .focus
                .iter()
                .any(|(id, focus)| id.starts_with("sc-model-") && focus.is_focused(window));
            if model_focused {
                let id = if self.pane == Pane::SmartCommit {
                    format!("sc-provider-{}", self.snapshot.smart_provider.slug())
                } else {
                    format!("settings-pane-{}", self.pane.id())
                };
                let focus = self.handle(&id, cx);
                window.focus(&focus, cx);
            }
        }
    }
}
