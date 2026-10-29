//! Settings controls of the kit: switch, segmented, pop-up value, stepper,
//! capsule button, slider, checkbox, progress bar.
use super::*;

/// The mini switch of a grouped form (HIG `toggles.md › Switches`).
pub fn switch(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    forced: Forced,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    let wrapper_id = SharedString::from(format!("{id}-key"));
    let inner = gpui_component::switch::Switch::new(id)
        .checked(checked)
        .small()
        .color(ink::accent())
        .disabled(forced == Forced::Disabled)
        .on_click(move |next: &bool, window, cx| on_toggle(*next, window, cx));
    ring(
        div().id(wrapper_id).flex().items_center(),
        forced == Forced::Focus,
        12.,
    )
    .role(Role::Switch)
    .aria_label(label)
    .aria_toggled(if checked {
        gpui::Toggled::True
    } else {
        gpui::Toggled::False
    })
    .child(inner)
}

/// Text-only segmented control with equal-width segments
/// (HIG `segmented-controls.md › Content`). Build with [`Segmented::new`].
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    labels: Vec<SharedString>,
    selected: usize,
    forced: Option<(usize, Forced)>,
    full_width: bool,
    on_select: Option<SelectHandler>,
}

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

impl Segmented {
    pub fn new(
        id: impl Into<ElementId>,
        labels: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            selected,
            forced: None,
            full_width: false,
            on_select: None,
        }
    }
    /// Draw `segment` in `state` (gallery captures).
    pub fn forced(mut self, segment: usize, state: Forced) -> Self {
        self.forced = Some((segment, state));
        self
    }
    /// Span the container (a two-choice switcher heading a group); segments
    /// stay equal.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let widest = self
            .labels
            .iter()
            .map(|label| {
                label
                    .chars()
                    .map(|c| if c.is_ascii() { 1. } else { 1.8 })
                    .sum::<f32>()
            })
            .fold(0_f32, f32::max);
        let segment_w = pt(widest * 6.6 + 22.);
        let track = div()
            .id(self.id.clone())
            .h(pt(CONTROL_HEIGHT))
            .p(pt(2.))
            .flex()
            .rounded(pt(CONTROL_RADIUS))
            .bg(rgb(ink::control()))
            .role(Role::RadioGroup)
            .when(self.full_width, |el| el.w_full());
        let mut track = track;
        for (index, label) in self.labels.into_iter().enumerate() {
            let selected = index == self.selected;
            let forced = self
                .forced
                .filter(|(segment, _)| *segment == index)
                .map_or(Forced::Rest, |(_, state)| state);
            let on_select = self.on_select.clone();
            let cell = div()
                .id(ElementId::NamedInteger("segment".into(), index as u64))
                .when(self.full_width, |el| el.flex_1().min_w_0())
                .when(!self.full_width, |el| el.w(segment_w))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(pt(CONTROL_RADIUS - 2.))
                .role(Role::RadioButton)
                .aria_label(label.clone())
                .aria_selected(selected)
                .when(selected, |el| {
                    el.bg(ink::selected())
                        .font_weight(FontWeight::MEDIUM)
                        .shadow_sm()
                })
                .when(!selected, |el| el.text_color(ink::secondary()))
                .when_some(on_select, |el, f| {
                    el.on_click(move |_, window, cx| f(index, window, cx))
                })
                .child(label);
            let cell = ring(cell, forced == Forced::Focus, CONTROL_RADIUS - 2.);
            track = track.child(if selected {
                cell.when(forced == Forced::Disabled, |el| {
                    el.opacity(ink::disabled_opacity())
                })
            } else {
                interactive_fill(cell, forced, None)
            });
        }
        track
    }
}

/// A pop-up button's closed look: current value and up/down chevrons
/// (HIG `pop-up-buttons.md`). The menu itself belongs to the caller.
pub fn popup_value(
    id: impl Into<ElementId>,
    value: impl Into<SharedString>,
    forced: Forced,
) -> Stateful<Div> {
    let value = value.into();
    let el = div()
        .id(id)
        .h(pt(CONTROL_HEIGHT))
        .pl(pt(8.) - px(RING))
        .pr(pt(3.) - px(RING))
        .flex()
        .items_center()
        .gap(pt(8.))
        .role(Role::ComboBox)
        .aria_label(value.clone())
        .child(value)
        .child(
            div()
                .size(pt(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(pt(5.))
                .bg(rgb(ink::control()))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .with_size(Size::Size(pt(12.)))
                        .text_color(ink::secondary()),
                ),
        );
    interactive_fill(
        ring(el, forced == Forced::Focus, CONTROL_RADIUS),
        forced,
        None,
    )
}

/// Which stepper button a [`Forced`] state applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepperPart {
    Decrease,
    Increase,
}

/// Compact stepper: − value +. A button at its limit is drawn disabled.
pub fn stepper(
    id: &'static str,
    value: impl Into<SharedString>,
    can_decrease: bool,
    can_increase: bool,
    forced: Option<(StepperPart, Forced)>,
) -> Div {
    let button = |part: StepperPart, icon: IconName, enabled: bool| {
        let state = match forced {
            Some((p, state)) if p == part => state,
            _ if !enabled => Forced::Disabled,
            _ => Forced::Rest,
        };
        let state = if enabled { state } else { Forced::Disabled };
        let suffix = if part == StepperPart::Decrease {
            "dec"
        } else {
            "inc"
        };
        let el = div()
            .id(SharedString::from(format!("{id}-{suffix}")))
            .size(pt(CONTROL_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .rounded(pt(6.))
            .role(Role::Button)
            .aria_label(if part == StepperPart::Decrease {
                "−"
            } else {
                "+"
            })
            .child(Icon::new(icon).with_size(Size::Size(pt(12.))));
        interactive_fill(
            ring(el, state == Forced::Focus, 6.),
            state,
            Some(ink::control()),
        )
    };
    div()
        .flex()
        .items_center()
        .gap(pt(8.))
        .child(button(StepperPart::Decrease, IconName::Minus, can_decrease))
        .child(
            div()
                .id(SharedString::from(format!("{id}-value")))
                .min_w(pt(40.))
                .flex()
                .justify_center()
                .role(Role::Status)
                .child(value.into()),
        )
        .child(button(StepperPart::Increase, IconName::Plus, can_increase))
}

/// Neutral capsule button for a row action ("Import", "Sign In").
pub fn capsule_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    forced: Forced,
) -> Stateful<Div> {
    let label = label.into();
    let el = div()
        .id(id)
        .h(pt(CONTROL_HEIGHT))
        .px(pt(12.) - px(RING))
        .flex()
        .items_center()
        .rounded(pt(CONTROL_HEIGHT / 2.))
        .role(Role::Button)
        .aria_label(label.clone())
        .child(label);
    interactive_fill(
        ring(el, forced == Forced::Focus, CONTROL_HEIGHT / 2.),
        forced,
        Some(ink::control()),
    )
}

/// What sits at a [`Slider`]'s ends: a symbol beside the track, or a caption
/// under each end ("Left" / "Right").
pub enum SliderEnds {
    None,
    Symbols(IconName, IconName),
    Captions(SharedString, SharedString),
}

/// A horizontal slider. Every indicator — fill, thumb and tick marks — is
/// placed by the same value → x mapping, so the thumb always sits on its
/// value and on a tick when `ticks` divides the range. Build with
/// [`Slider::new`]; `value` is a fraction in 0.0–1.0.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    ticks: usize,
    ends: SliderEnds,
    width: f32,
    forced: Forced,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            id: id.into(),
            value: value.clamp(0., 1.),
            ticks: 0,
            ends: SliderEnds::None,
            width: 200.,
            forced: Forced::Rest,
        }
    }
    /// Draw `count` evenly spaced tick marks under the track (0 = none).
    pub fn ticks(mut self, count: usize) -> Self {
        self.ticks = count;
        self
    }
    pub fn ends(mut self, ends: SliderEnds) -> Self {
        self.ends = ends;
        self
    }
    /// Track width in design points (default 200).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    pub fn forced(mut self, forced: Forced) -> Self {
        self.forced = forced;
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        const THUMB: f32 = 18.;
        const TRACK: f32 = 4.;
        // One mapping for every indicator: the thumb's centre travels over
        // the track minus half a thumb at each end.
        let travel = self.width - THUMB;
        let x = |fraction: f32| THUMB / 2. + travel * fraction;
        let tick_count = self.ticks;
        let ticks = (0..tick_count).map(move |i| {
            let fraction = if tick_count > 1 {
                i as f32 / (tick_count - 1) as f32
            } else {
                0.
            };
            div()
                .absolute()
                .top(pt(THUMB + 3.))
                .left(pt(x(fraction) - 1.))
                .size(pt(2.))
                .rounded_full()
                .bg(ink::secondary())
        });
        let thumb = div()
            .absolute()
            .top_0()
            .left(pt(x(self.value) - THUMB / 2.))
            .size(pt(THUMB))
            .rounded_full()
            .bg(gpui::white())
            // A white thumb on a white light-theme card needs its own edge.
            .border_1()
            .border_color(ink::secondary().opacity(0.55))
            .shadow_md();
        // `ring` owns the border, so only a focused thumb trades its edge for it.
        let thumb = if self.forced == Forced::Focus {
            ring(thumb, true, THUMB / 2.)
        } else {
            thumb
        };
        let track = div()
            .id(self.id.clone())
            .relative()
            .w(pt(self.width))
            .h(pt(THUMB + if tick_count > 0 { 6. } else { 0. }))
            .role(Role::Slider)
            .aria_label(format!("{:.0}%", self.value * 100.))
            .child(
                div()
                    .absolute()
                    .top(pt((THUMB - TRACK) / 2.))
                    .left(pt(THUMB / 2.))
                    .w(pt(travel))
                    .h(pt(TRACK))
                    .rounded_full()
                    .bg(rgb(ink::control_pressed())),
            )
            .child(
                div()
                    .absolute()
                    .top(pt((THUMB - TRACK) / 2.))
                    .left(pt(THUMB / 2.))
                    .w(pt(travel * self.value))
                    .h(pt(TRACK))
                    .rounded_full()
                    .bg(ink::accent()),
            )
            .children(ticks)
            .child(thumb)
            .when(self.forced == Forced::Disabled, |el| {
                el.opacity(ink::disabled_opacity())
            });
        let symbol = |icon: IconName| {
            Icon::new(icon)
                .with_size(Size::Size(pt(14.)))
                .text_color(ink::secondary())
        };
        match self.ends {
            SliderEnds::None => div().child(track),
            SliderEnds::Symbols(leading, trailing) => div()
                .flex()
                .items_start()
                .gap(pt(8.))
                .child(
                    div()
                        .h(pt(THUMB))
                        .flex()
                        .items_center()
                        .child(symbol(leading)),
                )
                .child(track)
                .child(
                    div()
                        .h(pt(THUMB))
                        .flex()
                        .items_center()
                        .child(symbol(trailing)),
                ),
            SliderEnds::Captions(leading, trailing) => {
                div().flex().flex_col().gap(pt(2.)).child(track).child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(pt(SECONDARY))
                        .text_color(ink::secondary())
                        .child(leading)
                        .child(trailing),
                )
            }
        }
    }
}

/// A checkbox with its label on the trailing side (HIG `toggles.md ›
/// Checkboxes`).
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    forced: Forced,
) -> Stateful<Div> {
    let label = label.into();
    let mark = div()
        .size(pt(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(pt(4.))
        .when(checked, |el| {
            el.bg(ink::accent())
                .text_color(hsla(theme().on_accent()))
                .child(Icon::new(IconName::Check).with_size(Size::Size(pt(12.))))
        })
        .when(!checked, |el| {
            el.border_1()
                .border_color(ink::secondary())
                .bg(rgb(ink::card()))
        });
    let el = div()
        .id(id)
        .h(pt(CONTROL_HEIGHT))
        .px(pt(4.) - px(RING))
        .flex()
        .items_center()
        .gap(pt(6.))
        .role(Role::CheckBox)
        .aria_label(label.clone())
        .aria_toggled(if checked {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .child(mark)
        .child(label);
    ring(el, forced == Forced::Focus, 6.).when(forced == Forced::Disabled, |el| {
        el.opacity(ink::disabled_opacity())
    })
}

/// A determinate progress bar filling its container's width.
pub fn progress_bar(fraction: f32, tint: Tint) -> Div {
    let fraction = fraction.clamp(0., 1.);
    div()
        .w_full()
        .h(pt(6.))
        .rounded_full()
        .bg(rgb(ink::control_pressed()))
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded_full()
                .bg(rgb(tint.color())),
        )
}
