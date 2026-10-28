//! Final-kit role geometry around the shared production interaction contract.
use super::kit_style::{ink, metrics, pt};
use super::*;
use gpui_component::{Icon, IconName, Sizable, Size};
use std::time::Duration;

impl SettingsView {
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
        let visual = div()
            .id(SharedString::from(format!("{id}-visual")))
            .relative()
            .min_h(pt(metrics::CONTROL_HEIGHT))
            .min_w_0()
            .max_w_full()
            .px(pt(12.))
            .py(pt(2.))
            .line_height(pt(20.))
            .rounded(pt(12.))
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(ink::control()))
            .opacity(if enabled { 1. } else { ink::disabled_opacity() })
            .when(enabled, |el| {
                el.group_hover(SharedString::from(id.to_owned()), |s| {
                    s.bg(rgb(ink::control_hover()))
                })
                .group_active(SharedString::from(id.to_owned()), |s| {
                    s.bg(rgb(ink::control_pressed()))
                })
            })
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(self.measured(
                    format!("{id}-visual"),
                    Role::Image,
                    label.to_string(),
                    String::new(),
                    false,
                    false,
                    !enabled,
                ))
            })
            .child(label.clone());
        self.control(
            id.into(),
            label,
            false,
            enabled,
            Role::Button,
            focus_background,
            px(0.),
            15.,
            Some(intent),
            None,
            window,
            cx,
        )
        .min_h(pt(metrics::CONTROL_HIT_HEIGHT))
        .max_w_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(visual)
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
        let visual = div()
            .id(SharedString::from(format!("{id}-visual")))
            .relative()
            .size(pt(metrics::CONTROL_HEIGHT))
            .rounded(pt(6.))
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(ink::control()))
            .opacity(if enabled { 1. } else { ink::disabled_opacity() })
            .when(enabled, |el| {
                el.group_hover(SharedString::from(id.to_owned()), |s| {
                    s.bg(rgb(ink::control_hover()))
                })
                .group_active(SharedString::from(id.to_owned()), |s| {
                    s.bg(rgb(ink::control_pressed()))
                })
            })
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(self.measured(
                    format!("{id}-visual"),
                    Role::Image,
                    label.to_string(),
                    String::new(),
                    false,
                    false,
                    !enabled,
                ))
            })
            .child(
                Icon::new(if id.ends_with("-inc") {
                    IconName::Plus
                } else {
                    IconName::Minus
                })
                .with_size(Size::Size(pt(12.))),
            );
        self.control(
            id.into(),
            label,
            false,
            enabled,
            Role::Button,
            ink::card(),
            px(0.),
            9.,
            Some(intent),
            None,
            window,
            cx,
        )
        .size(pt(metrics::CONTROL_HIT_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .child(visual)
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
        let visual = SwitchVisual {
            id: id.to_owned().into(),
            checked,
            accent: self.snapshot.effective_accent,
            reduce_motion: self.snapshot.reduce_motion || kagi_ui_core::theme::reduce_motion(),
            track_probe: cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    format!("{id}-visual"),
                    Role::Image,
                    title.t().into(),
                    String::new(),
                    false,
                    checked,
                    false,
                )
            }),
            knob_probe: cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    format!("{id}-knob"),
                    Role::Image,
                    String::new(),
                    String::new(),
                    false,
                    checked,
                    false,
                )
            }),
        };
        self.control(
            id.into(),
            title.t().into(),
            checked,
            true,
            Role::Switch,
            ink::card(),
            px(0.),
            17.,
            Some(intent),
            None,
            window,
            cx,
        )
        .w(pt(32.))
        .h(pt(metrics::CONTROL_HIT_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .child(visual)
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
        if !ids.iter().any(|id| {
            self.focus
                .get(id)
                .is_some_and(|focus| focus.is_focused(window))
        }) {
            let selected = entries
                .iter()
                .position(|entry| entry.2 && entry.3)
                .or_else(|| entries.iter().position(|entry| entry.3))
                .unwrap_or(0);
            self.groups.insert(group.into(), selected);
        }
        // The final kit derives a shared width from the labels, not a page-wide bank.
        let mut font = kagi_ui_core::theme::ui_font();
        font.weight = FontWeight::MEDIUM;
        let widest = entries
            .iter()
            .map(|entry| {
                window
                    .text_system()
                    .shape_line(
                        entry.1.clone(),
                        pt(metrics::BODY),
                        &[TextRun {
                            len: entry.1.len(),
                            font: font.clone(),
                            color: ink::text(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                    .width()
            })
            .fold(px(0.), Pixels::max);
        let segment_width = (widest + pt(22.)).max(pt(metrics::CONTROL_HIT_HEIGHT));
        // Corrected focus decoration needs six physical pixels between siblings.
        let gap = pt(2.).max(px(6.));
        let width = segment_width * entries.len() as f32
            + pt(4.)
            + gap * entries.len().saturating_sub(1) as f32;
        let mut track = div()
            .id(SharedString::from(group.to_owned()))
            .relative()
            .min_w_0()
            .w(width)
            .max_w_full()
            .flex()
            .flex_wrap()
            .gap(gap)
            .px(pt(2.))
            .role(Role::RadioGroup)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(pt(2.))
                    .bottom(pt(2.))
                    .rounded(pt(metrics::CONTROL_RADIUS))
                    .bg(rgb(ink::control()))
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        el.child(self.measured(
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
            let visual = div()
                .id(SharedString::from(format!("{id}-visual")))
                .absolute()
                .left_0()
                .right_0()
                .top(pt(4.))
                .bottom(pt(4.))
                .rounded(pt(5.))
                .when(selected, |el| el.bg(ink::selected()).shadow_sm())
                .when(enabled && !selected, |el| {
                    el.group_hover(SharedString::from(id.clone()), |s| {
                        s.bg(rgb(ink::control_hover()))
                    })
                    .group_active(SharedString::from(id.clone()), |s| {
                        s.bg(rgb(ink::control_pressed()))
                    })
                })
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(self.measured(
                        format!("{id}-visual"),
                        Role::Image,
                        label.to_string(),
                        String::new(),
                        false,
                        selected,
                        !enabled,
                    ))
                });
            let label_element = div()
                .relative()
                .min_w_0()
                .font_weight(if selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if selected {
                    ink::text()
                } else {
                    ink::secondary()
                })
                .opacity(if enabled { 1. } else { ink::disabled_opacity() })
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(self.measured(
                        format!("{id}-label"),
                        Role::Label,
                        label.to_string(),
                        String::new(),
                        false,
                        selected,
                        !enabled,
                    ))
                })
                .child(label.clone());
            let item = self
                .control(
                    id,
                    label,
                    selected,
                    enabled,
                    Role::RadioButton,
                    ink::card(),
                    px(0.),
                    8.,
                    Some(intent),
                    Some((group.into(), index, ids.clone())),
                    window,
                    cx,
                )
                .flex_grow(1.)
                .flex_shrink(1.)
                .flex_basis(segment_width)
                .min_w(pt(metrics::CONTROL_HIT_HEIGHT))
                .px(pt(8.))
                .py(pt(4.))
                .line_height(pt(20.))
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .child(visual)
                .child(label_element);
            track = track.child(item);
        }
        track.into_any_element()
    }
}

#[derive(IntoElement)]
struct SwitchVisual {
    id: SharedString,
    checked: bool,
    accent: u32,
    reduce_motion: bool,
    track_probe: Option<AnyElement>,
    knob_probe: Option<AnyElement>,
}
struct SwitchMotion {
    previous: bool,
    target: bool,
    task: Option<Task<()>>,
}
impl RenderOnce for SwitchVisual {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let state = window.use_keyed_state(
            SharedString::from(format!("{}-motion", self.id)),
            cx,
            |_, _| SwitchMotion {
                previous: checked,
                target: checked,
                task: None,
            },
        );
        if self.reduce_motion {
            state.update(cx, |state, _| {
                state.previous = checked;
                state.target = checked;
                state.task = None;
            });
        } else if state.read(cx).target != checked {
            state.update(cx, |state, _| {
                state.previous = state.target;
                state.target = checked;
                state.task = None;
            });
            let weak = state.downgrade();
            let task = cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let _ = weak.update(cx, |state, cx| {
                    state.previous = state.target;
                    cx.notify();
                });
            });
            state.update(cx, |state, _| state.task = Some(task));
        }
        let moving = state.read(cx).previous != checked;
        let knob = div()
            .relative()
            .top(pt(2.))
            .size(pt(12.))
            .rounded_full()
            .bg(white())
            .shadow_md()
            .children(self.knob_probe);
        let knob = if moving {
            knob.with_animation(
                ElementId::NamedInteger("settings-switch-move".into(), checked as u64),
                Animation::new(Duration::from_millis(150)),
                move |knob, delta| {
                    knob.left(pt(2.
                        + if checked {
                            12. * delta
                        } else {
                            12. * (1. - delta)
                        }))
                },
            )
            .into_any_element()
        } else {
            knob.left(pt(if checked { 14. } else { 2. }))
                .into_any_element()
        };
        div()
            .id(SharedString::from(format!("{}-visual", self.id)))
            .relative()
            .w(pt(28.))
            .h(pt(16.))
            .rounded_full()
            .bg(rgb(if checked { self.accent } else { ink::control() }))
            .children(self.track_probe)
            .child(knob)
    }
}
