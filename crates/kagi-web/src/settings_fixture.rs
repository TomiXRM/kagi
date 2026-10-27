//! Verification adapter, not a second Settings renderer or browser product.
use gpui::{div, prelude::*, Context, Entity, Subscription, Window};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use kagi_ui_settings::{SettingsEvent, SettingsSnapshot, SettingsView};
use std::cell::RefCell;

thread_local! {
    static OBSERVED: RefCell<(Lang, usize)> = const { RefCell::new((Lang::En, 0)) };
}

pub fn observed() -> String {
    OBSERVED.with(|state| {
        let (language, requests) = *state.borrow();
        format!(
            "{{\"language\":\"{}\",\"requests\":{requests}}}",
            language.slug()
        )
    })
}

pub struct SettingsFixture {
    view: Entity<SettingsView>,
    focus: gpui::FocusHandle,
    _events: Subscription,
}

impl SettingsFixture {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        i18n::set_lang_no_persist(Lang::En);
        theme::init_loaded_active("apple-light");
        theme::init_loaded_zoom(1.);
        theme::sync_gpui_component_theme(cx);
        let view = cx.new(|_| SettingsView::new(SettingsSnapshot { language: Lang::En }));
        let events = cx.subscribe(&view, |_, view, event, cx| match *event {
            SettingsEvent::LanguageRequested(language) => {
                OBSERVED.with(|state| {
                    let mut state = state.borrow_mut();
                    state.0 = language;
                    state.1 += 1;
                });
                i18n::set_lang_no_persist(language);
                view.update(cx, |view, cx| {
                    view.seed_effective(SettingsSnapshot { language }, cx);
                });
                cx.refresh_windows();
            }
        });
        Self {
            view,
            focus,
            _events: events,
        }
    }
}

impl gpui::Render for SettingsFixture {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(theme::scaled_px(16.));
        div()
            .id("production-settings-fixture")
            .track_focus(&self.focus)
            .size_full()
            .p_5()
            .bg(gpui::rgb(theme::theme().panel))
            .text_color(gpui::rgb(theme::theme().text_main))
            .font(theme::ui_font())
            .child(self.view.clone())
    }
}
