//! The single production Settings renderer for native and browser hosts.
//! Hosts own effects; effective seeds never emit requests or overwrite drafts.
mod analyze;
mod controls;
mod focus_ring;
mod model;
mod panes;
mod theme_cards;
use gpui::{prelude::*, *};
use gpui_component::input::{InputEvent, InputState};
use kagi_ui_core::i18n::Msg;
pub use model::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub type DetailSlot = Rc<dyn Fn(&mut Window, &mut App) -> Option<AnyElement>>;
pub type ContentMeasure = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App)>;
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct ControlProbe {
    pub id: String,
    pub bounds: Bounds<Pixels>,
    pub focused: bool,
    pub selected: bool,
    pub disabled: bool,
    pub role: Role,
    pub name: String,
    pub value: String,
    pub paint: Option<ControlPaint>,
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, Debug)]
pub struct ControlPaint {
    pub font_size: Pixels,
    pub font_weight: FontWeight,
    pub text_color: Hsla,
}

pub struct SettingsView {
    snapshot: SettingsSnapshot,
    pane: Pane,
    input: Entity<InputState>,
    revision: u64,
    analyze: analyze::AnalyzeState,
    focus: BTreeMap<String, FocusHandle>,
    revealed: BTreeMap<String, Rc<std::cell::Cell<bool>>>,
    groups: BTreeMap<String, usize>,
    scroll: ScrollHandle,
    theme_scroll: ScrollHandle,
    // Last presentation request, not another source of selected-theme state.
    theme_reveal: Option<(SharedString, usize, Option<usize>, f32, usize)>,
    theme_viewport: Rc<std::cell::Cell<Size<Pixels>>>,
    detail_slot: Option<DetailSlot>,
    content_measure: Option<ContentMeasure>,
    detail_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    chrome_heights: Rc<RefCell<(Pixels, Pixels)>>,
    #[cfg(feature = "gui-e2e")]
    probes: Rc<RefCell<BTreeMap<String, ControlProbe>>>,
    _input_subscription: Subscription,
}
impl EventEmitter<SettingsEvent> for SettingsView {}
impl SettingsView {
    pub fn new(
        snapshot: SettingsSnapshot,
        input: Entity<InputState>,
        pane: Pane,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&input, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.revision += 1;
                this.analyze.saved_feedback = false;
                cx.notify();
            }
        });
        Self {
            analyze: analyze::AnalyzeState::new(snapshot.analyze_saved.clone()),
            snapshot,
            pane,
            input,
            revision: 0,
            focus: BTreeMap::new(),
            revealed: BTreeMap::new(),
            groups: BTreeMap::new(),
            scroll: ScrollHandle::new(),
            theme_scroll: ScrollHandle::new(),
            theme_reveal: None,
            theme_viewport: Rc::default(),
            detail_slot: None,
            content_measure: None,
            chrome_heights: Rc::new(RefCell::new((px(0.), px(0.)))),
            detail_bounds: Rc::default(),
            #[cfg(feature = "gui-e2e")]
            probes: Rc::default(),
            _input_subscription: subscription,
        }
    }
    pub fn snapshot(&self) -> &SettingsSnapshot {
        &self.snapshot
    }
    pub fn pane(&self) -> Pane {
        self.pane
    }
    /// Reset presentation on host reopen without replacing preferences or drafts.
    pub fn prepare_open(&mut self, cx: &mut Context<Self>) {
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.theme_reveal = None;
        cx.notify();
    }
    pub fn seed_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        if self.pane != pane {
            self.pane = pane;
            self.scroll.set_offset(point(px(0.), px(0.)));
            self.theme_reveal = None;
            cx.notify();
        }
    }
    pub fn seed_effective(&mut self, snapshot: SettingsSnapshot, cx: &mut Context<Self>) {
        if !self.snapshot.same_effective(&snapshot) {
            self.snapshot = snapshot;
            cx.notify();
        }
    }
    pub fn set_detail_slot(&mut self, slot: DetailSlot, cx: &mut Context<Self>) {
        self.detail_slot = Some(slot);
        cx.notify();
    }
    pub fn detail_viewport_bounds(&self) -> Option<Bounds<Pixels>> {
        *self.detail_bounds.borrow()
    }
    pub fn set_content_measure(&mut self, measure: ContentMeasure, cx: &mut Context<Self>) {
        self.content_measure = Some(measure);
        cx.notify();
    }
    pub fn scroll_handle(&self) -> ScrollHandle {
        self.scroll.clone()
    }
    #[cfg(feature = "gui-e2e")]
    pub fn control_probe(&self, id: &str) -> Option<ControlProbe> {
        self.probes.borrow().get(id).cloned()
    }
    #[cfg(feature = "gui-e2e")]
    pub fn focus_control(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(focus) = self.focus.get(id) else {
            return false;
        };
        window.focus(focus, cx);
        cx.notify();
        true
    }
    #[cfg(feature = "gui-e2e")]
    pub fn control_probes(&self) -> Vec<ControlProbe> {
        self.probes.borrow().values().cloned().collect()
    }
    pub fn analyze_request(&self, cx: &App) -> AnalyzeRequest {
        AnalyzeRequest {
            text: self.input.read(cx).value(),
            revision: self.revision,
        }
    }
    pub fn analyze_dirty(&self, cx: &App) -> bool {
        self.input.read(cx).value() != self.analyze.baseline
    }
    pub fn analyze_error(&self) -> Option<&str> {
        self.analyze.error.as_deref()
    }
    pub fn analyze_baseline(&self) -> &str {
        &self.analyze.baseline
    }
    pub fn seed_analyze_result(
        &mut self,
        result: AnalyzeResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.analyze_request(cx);
        if let Some(text) = self.analyze.acknowledge(result, &current) {
            self.input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        cx.notify();
    }
    fn emit(&mut self, intent: SettingsIntent, window: &mut Window, cx: &mut Context<Self>) {
        let intent = match intent {
            SettingsIntent::SaveAnalyze(_) => SettingsIntent::SaveAnalyze(self.analyze_request(cx)),
            SettingsIntent::ResetAnalyze(_) => {
                SettingsIntent::ResetAnalyze(self.analyze_request(cx))
            }
            other => other,
        };
        if let SettingsIntent::Pane(pane) = intent {
            self.seed_pane(pane, cx);
        }
        cx.emit(SettingsEvent {
            window: window.window_handle(),
            intent,
        });
    }
}
impl Pane {
    pub fn title(self) -> &'static str {
        match self {
            Self::Appearance => Msg::SettingsAppearance,
            Self::Graph => Msg::SettingsPaneGraph,
            Self::Editor => Msg::SettingsPaneEditor,
            Self::Terminal => Msg::SettingsPaneTerminal,
            Self::Network => Msg::SettingsPaneNetwork,
            Self::SmartCommit => Msg::SettingsPaneSmartCommit,
            Self::Analyze => Msg::SettingsPaneAnalyze,
            Self::General => Msg::SettingsGeneral,
        }
        .t()
    }
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "gui-e2e")]
        self.probes.borrow_mut().clear();
        self.repair_focus(window, cx);
        let sidebar = self.sidebar(window, cx);
        let content = self.render_pane(window, cx);
        let measure = self.content_measure.clone();
        let chrome = self.chrome_heights.clone();
        let content = div().relative().min_w_0().w_full().child(content).child(
            canvas(
                move |mut bounds, window, cx| {
                    let (title, sidebar) = *chrome.borrow();
                    bounds.size.height =
                        (bounds.size.height + title + controls::pt(36.)).max(sidebar);
                    if let Some(measure) = measure {
                        measure(bounds, window, cx);
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        let bounds = self.detail_bounds.clone();
        let slot = self.detail_slot.as_ref().and_then(|slot| slot(window, cx));
        let title_heights = self.chrome_heights.clone();
        let viewport = div()
            .id("settings-detail-viewport")
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                canvas(
                    move |rect, _, _| {
                        *bounds.borrow_mut() = Some(rect);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .id("settings-detail-scrollbar")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("settings-detail-scroll")
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .map(|mut detail| {
                                detail.style().restrict_scroll_to_axis = Some(true);
                                detail
                            })
                            .p(controls::pt(18.))
                            .child(content),
                    )
                    // Measure the fixed viewport, not a child translated by scroll.
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            "settings-detail-scroll".into(),
                            Role::Group,
                            self.pane.title().into(),
                            String::new(),
                            false,
                            false,
                            false,
                        )
                    }))
                    .child(
                        gpui_component::scroll::Scrollbar::vertical(&self.scroll)
                            .id("settings-detail-scrollbar"),
                    ),
            )
            .children(slot);
        div()
            .id("settings-production")
            .size_full()
            // Hosts place fixed chrome above this view; use only the remainder.
            .flex_1()
            .min_h_0()
            .flex()
            .bg(rgb(kagi_ui_core::theme::theme().panel))
            .text_color(rgb(kagi_ui_core::theme::theme().text_main))
            .text_size(controls::pt(13.))
            .font(kagi_ui_core::theme::ui_font())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let direction = match event.keystroke.key.as_str() {
                    "pageup" => 1.,
                    "pagedown" => -1.,
                    _ => return,
                };
                // The editor owns caret/selection paging. Only unconsumed
                // page keys outside it move the surrounding detail viewport.
                if this.input.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                let offset = this.scroll.offset();
                let page = this.scroll.bounds().size.height * 0.9;
                this.scroll.set_offset(point(
                    offset.x,
                    (offset.y + page * direction).clamp(-this.scroll.max_offset().y, px(0.)),
                ));
                cx.stop_propagation();
                cx.notify();
            }))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("settings-pane-title")
                            .relative()
                            .role(Role::Heading)
                            .flex_shrink_0()
                            .px(controls::pt(18.))
                            .py(controls::pt(14.))
                            .child(
                                canvas(
                                    move |bounds, _, _| {
                                        title_heights.borrow_mut().0 = bounds.size.height;
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                            .font_weight(FontWeight::BOLD)
                            .child(self.pane.title())
                            .children(cfg!(feature = "gui-e2e").then(|| {
                                self.measured(
                                    "settings-pane-title".into(),
                                    Role::Heading,
                                    self.pane.title().into(),
                                    String::new(),
                                    false,
                                    false,
                                    false,
                                )
                            })),
                    )
                    .child(viewport),
            )
    }
}
