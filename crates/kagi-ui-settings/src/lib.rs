//! Production Settings input/rendering, shared by native and browser hosts.
//! Effects remain in host adapters; seeding effective values never emits intent.
use gpui::{div, prelude::*, Context, EventEmitter, SharedString, Window};
use gpui_component::radio::RadioGroup;
use kagi_ui_core::{
    i18n::{Lang, Msg},
    theme::{self, theme},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub language: Lang,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    LanguageRequested(Lang),
}

/// Retained by the application/fixture, not by a disposable native window.
pub struct SettingsView {
    snapshot: SettingsSnapshot,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub fn new(snapshot: SettingsSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn seed_effective(&mut self, snapshot: SettingsSnapshot, cx: &mut Context<Self>) {
        if self.snapshot != snapshot {
            self.snapshot = snapshot;
            cx.notify();
        }
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        self.snapshot
    }
}

impl gpui::Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = usize::from(self.snapshot.language == Lang::Ja);
        div()
            .id("settings-language-production")
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(theme::scaled_px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(theme().text_sub))
                    .pb_1()
                    .border_b_1()
                    .border_color(gpui::rgb(theme().elevated_border(theme().surface)))
                    .child(Msg::SettingsLanguage.t()),
            )
            .child(
                div()
                    .id("settings-language-group")
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_2()
                    .rounded(gpui::px(8.))
                    .bg(gpui::rgb(theme().bg_base))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_color(gpui::rgb(theme().text_main))
                            .child(Msg::SettingsInterfaceLang.t())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(gpui::rgb(theme().text_sub))
                                    .child(Msg::SettingsInterfaceLangDesc.t()),
                            ),
                    )
                    .child(
                        RadioGroup::horizontal("settings-language")
                            .children([SharedString::from("English"), SharedString::from("日本語")])
                            .selected_index(Some(selected))
                            .on_click(cx.listener(|_, index: &usize, _, cx| {
                                let language = match index {
                                    0 => Lang::En,
                                    1 => Lang::Ja,
                                    _ => return,
                                };
                                cx.emit(SettingsEvent::LanguageRequested(language));
                            })),
                    ),
            )
    }
}
