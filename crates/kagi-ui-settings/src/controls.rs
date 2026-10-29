//! Shared interaction, grouped rows and focus repair for the final settings kit.
use super::kit_style::{ink, metrics, pt};
use super::*;
use gpui_component::ElementExt as _;
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
            if self.rows > 0 {
                let separator = div().h(px(1.)).flex_shrink_0().bg(rgb(ink::hairline()));
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
                self.container.extend([separator.into_any_element()]);
            }
            #[cfg(feature = "gui-e2e")]
            let element = MeasuredGroupPart {
                element,
                id: format!("{}-row-{}", self.id, self.rows),
                probes: self.probes.clone(),
            }
            .into_any_element();
            self.container.extend([element]);
            self.rows += 1;
        }
    }
}

impl RenderOnce for SettingsGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        #[cfg(feature = "gui-e2e")]
        return MeasuredGroupPart {
            element: self.container.into_any_element(),
            id: self.id.into(),
            probes: self.probes,
        }
        .into_any_element();
        #[cfg(not(feature = "gui-e2e"))]
        self.container.into_any_element()
    }
}

/// Transparent test recorder: forwards the child's LayoutId, without introducing
/// a measuring flex wrapper or reconstructing its border box from style values.
#[cfg(feature = "gui-e2e")]
struct MeasuredGroupPart {
    element: AnyElement,
    id: String,
    probes: Rc<RefCell<BTreeMap<String, ControlProbe>>>,
}

#[cfg(feature = "gui-e2e")]
impl IntoElement for MeasuredGroupPart {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[cfg(feature = "gui-e2e")]
impl Element for MeasuredGroupPart {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.prepaint(window, cx);
        self.probes.borrow_mut().insert(
            self.id.clone(),
            ControlProbe {
                id: self.id.clone(),
                bounds,
                role: Role::Group,
                name: self.id.clone(),
                value: String::new(),
                paint: None,
                focused: false,
                selected: false,
                disabled: false,
            },
        );
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(window, cx);
    }
}

impl SettingsView {
    pub(crate) fn section(
        &self,
        title: Msg,
        subtitle: Option<AnyElement>,
        content: impl IntoElement,
    ) -> AnyElement {
        div()
            .relative()
            .id(SharedString::from(format!(
                "settings-section-{}",
                title.t()
            )))
            .role(Role::Group)
            .aria_label(title.t())
            .min_w_0()
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    format!("settings-section-{}", title.t()),
                    Role::Group,
                    title.t().into(),
                    String::new(),
                    false,
                    false,
                    false,
                )
            }))
            .flex()
            .flex_col()
            .gap(pt(8.))
            .child(
                div()
                    .px(pt(metrics::GROUP_PADDING_X) + px(if theme().dark { 0. } else { 1. }))
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(pt(2.))
                    .child(
                        div()
                            .relative()
                            .id(SharedString::from(format!(
                                "settings-heading-{}",
                                title.t()
                            )))
                            .role(Role::Heading)
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .font_weight(FontWeight::BOLD)
                            .child(title.t())
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    format!("settings-heading-{}", title.t()),
                                    Role::Heading,
                                    title.t().into(),
                                    String::new(),
                                    false,
                                    false,
                                    false,
                                )
                            })),
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
    pub(crate) fn group(&self, id: &'static str) -> SettingsGroup {
        let container = div()
            .id(id)
            .relative()
            .min_w_0()
            .w_full()
            .rounded(pt(metrics::GROUP_RADIUS))
            .px(pt(metrics::GROUP_PADDING_X))
            .when(!theme().dark, |card| {
                card.border_1().border_color(rgb(ink::hairline()))
            })
            .bg(rgb(ink::card()))
            .flex()
            .flex_col();
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
        let pointer_id = (role == Role::Tab).then(|| id.clone());
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
        // The sidebar paints its ring after its tightly spaced row siblings,
        // within that local scroll owner, never as a window-level overlay.
        let focus_overlay = (focused && role != Role::Tab).then(|| {
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
            .when_some(pointer_id, |el, id| {
                // GPUI focuses a tracked element on mouse down, before click:
                // mark it here so the press itself never paints a ring.
                el.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, _| {
                        this.sidebar_pointer_focus = Some(id.clone());
                    }),
                )
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
                if !event.keystroke.modifiers.platform {
                    this.sidebar_pointer_focus = None;
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
