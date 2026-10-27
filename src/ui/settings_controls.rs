//! Settings choice focus and Analyze draft ownership (#1298 / #1299).
use super::{
    i18n::Msg,
    keyboard_nav::{self, TabFocus, TabList},
    settings,
    smart_commit::{SmartCommitState, SmartProvider},
    theme::theme,
    KagiApp, ToastKind,
};
use gpui::{div, prelude::*, rgb, App, Context, Entity, SharedString, Window};

#[derive(Default)]
pub(crate) struct SettingsControls {
    pub provider: TabFocus,
    pub view: Option<Entity<kagi_ui_settings::SettingsView>>,
    pub models: TabFocus,
    pub saved_ignore: Option<String>,
    pub ignore_error: Option<String>,
    pub(super) reset_alert: Option<super::settings_analyze::AnalyzeResetAlert>,
    pub(super) smart_consent: Option<SmartConsentAlert>,
}

/// Settings form approval, not a replacement for the main window's modal slot.
#[derive(Clone)]
pub(super) struct SmartConsentAlert {
    pub window: gpui::WindowId,
    pub focus: gpui::FocusHandle,
    pub alert_return_focus: super::overlay_focus::AlertReturnFocus,
}

impl SettingsControls {
    pub fn repair_focus(
        &self,
        shown: bool,
        models_shown: bool,
        fallback: Option<&gpui::FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.provider.release_closed(fallback, window, cx);
        self.models.release_closed(fallback, window, cx);
        if !shown {
            self.provider.yield_focus(fallback, window, cx);
        }
        if !models_shown {
            self.models.yield_focus(fallback, window, cx);
        }
    }
}

pub(super) fn provider_chips(
    app: &Entity<KagiApp>,
    smart: &SmartCommitState,
    state: &SettingsControls,
    pointer_focus: Option<gpui::FocusHandle>,
    cx: &App,
) -> impl IntoElement {
    let entries: Vec<_> = std::iter::once((
        SmartProvider::Ollama.display_name().to_string(),
        SmartProvider::Ollama,
        true,
    ))
    .chain(
        kagi_git::message_gen::CliProvider::ALL
            .into_iter()
            .map(|p| {
                (
                    SmartProvider::Cli(p).display_name().to_string(),
                    SmartProvider::Cli(p),
                    smart.cli_available_for(p),
                )
            }),
    )
    .collect();
    let selected = entries.iter().position(|(_, p, _)| *p == smart.provider);
    let owner = app.downgrade();
    let choices = entries.clone();
    let tabs = TabList::new(
        &state.provider,
        keyboard_nav::slot_keys(entries.len()),
        (0..entries.len()).collect(),
        selected,
        keyboard_nav::Activation::Manual,
        pointer_focus,
        move |slot, _, _, cx| {
            if let Some((_, provider, true)) = choices.get(slot) {
                let _ = owner.update(cx, |app, cx| {
                    app.smart_commit.set_provider(*provider);
                    cx.notify();
                });
            }
        },
        cx,
    );
    tabs.list(
        div()
            .id("settings-provider-choices")
            .aria_label(Msg::SettingsSmartProvider.t())
            .flex()
            .min_w_0()
            .w_full()
            .flex_wrap()
            .gap_2()
            .justify_end(),
    )
    .children(
        entries
            .into_iter()
            .enumerate()
            .map(|(slot, (label, provider, available))| {
                let hint = if available {
                    label.clone()
                } else {
                    super::i18n::provider_not_found_hint(&label)
                };
                let id = format!("sc-provider-{}", provider.slug());
                let chip = tabs
                    .cell(
                        slot,
                        &hint,
                        chip(&id, hint.clone(), selected == Some(slot), available),
                    )
                    .when(!available, |el| {
                        el.opacity(0.5).aria_description(SharedString::from(hint))
                    })
                    .when(!available, |el| {
                        el.a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                            builder.parent_node().set_disabled()
                        })
                    });
                chip.child(super::e2e::measure_inside(id))
            }),
    )
}

pub(super) fn model_chips(
    app: &Entity<KagiApp>,
    smart: &SmartCommitState,
    state: &SettingsControls,
    pointer_focus: Option<gpui::FocusHandle>,
    cx: &App,
) -> impl IntoElement {
    let models = smart.detected_models.clone();
    let selected = models
        .iter()
        .position(|m| Some(m.as_str()) == smart.model.as_deref());
    let choices = models.clone();
    let owner = app.downgrade();
    let tabs = TabList::new(
        &state.models,
        keyboard_nav::slot_keys(models.len()),
        (0..models.len()).collect(),
        selected,
        keyboard_nav::Activation::Manual,
        pointer_focus,
        move |slot, _, _, cx| {
            if let Some(model) = choices.get(slot) {
                let _ = owner.update(cx, |app, cx| {
                    app.smart_commit.set_model(model.clone());
                    cx.notify();
                });
            }
        },
        cx,
    );
    tabs.list(
        div()
            .id("settings-model-choices")
            .aria_label(Msg::SettingsSmartModel.t())
            .flex()
            .min_w_0()
            .w_full()
            .flex_wrap()
            .gap_2()
            .justify_end(),
    )
    .children(models.into_iter().enumerate().map(|(slot, model)| {
        let id = format!("sc-model-{model}");
        tabs.cell(
            slot,
            &model,
            chip(&id, model.clone(), selected == Some(slot), true),
        )
        .child(super::e2e::measure_inside(id))
    }))
}

fn chip(id: &str, label: String, selected: bool, available: bool) -> gpui::Stateful<gpui::Div> {
    // The original 1px selection outline is inside the out-of-flow focus ring.
    div()
        .id(SharedString::from(id.to_string()))
        .relative()
        .min_w_0()
        .max_w_full()
        .px(keyboard_nav::inset(12.) + gpui::px(1.))
        .py(keyboard_nav::inset(4.) + gpui::px(1.))
        .rounded_md()
        .bg(if selected {
            theme().segment_selected_bg()
        } else {
            rgb(theme().bg_base).into()
        })
        .text_sm()
        .text_color(if selected {
            rgb(theme().text_main)
        } else {
            rgb(theme().text_sub)
        })
        .when(available && !selected, |el| {
            el.hover(|s| {
                s.bg(rgb(theme().selected))
                    .text_color(rgb(theme().text_main))
                    .cursor_pointer()
            })
        })
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_md()
                .border_1()
                .border_color(rgb(if selected {
                    theme().ui_accent()
                } else {
                    theme().selected
                })),
        )
        .child(div().min_w_0().truncate().child(SharedString::from(label)))
}

impl KagiApp {
    pub(super) fn settings_smart_consent(&self) -> Option<&SmartConsentAlert> {
        self.settings_controls.smart_consent.as_ref()
    }

    pub(super) fn clear_settings_smart_consent(&mut self, window: gpui::WindowId) -> bool {
        if self
            .settings_smart_consent()
            .is_some_and(|alert| alert.window == window)
        {
            self.settings_controls.smart_consent = None;
            true
        } else {
            false
        }
    }

    pub(super) fn request_settings_smart_enabled(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !on || self.smart_commit.consent_accepted {
            self.smart_commit.set_enabled(on);
            if on {
                self.refresh_smart_commit_detection(cx);
            }
        } else if self.settings_smart_consent().is_none() {
            // Pointer activation does not focus the Switch's keyboard wrapper.
            // Both input paths must return to the same live Settings control.
            window.focus(&self.settings_switches.get(5, cx), cx);
            let focus = cx.focus_handle();
            self.settings_controls.smart_consent = Some(SmartConsentAlert {
                window: window.window_handle().window_id(),
                alert_return_focus: super::overlay_focus::AlertReturnFocus::for_settings(
                    window,
                    cx,
                    focus.clone(),
                ),
                focus: focus.clone(),
            });
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn finish_settings_smart_consent(
        &mut self,
        confirm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(alert) = self
            .settings_controls
            .smart_consent
            .take_if(|alert| alert.window == window.window_handle().window_id())
        else {
            return;
        };
        alert.alert_return_focus.restore_settings(window, cx);
        if confirm {
            self.smart_commit.accept_consent();
            kagi_ui_core::klog!("smart-commit: llm enabled (consent given)");
            self.refresh_smart_commit_detection(cx);
        }
        cx.notify();
    }

    pub(super) fn save_analyze_ignore(
        &mut self,
        reset: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !reset && !self.analyze_ignore_dirty(cx) {
            return;
        }
        let Some(input) = self.analyze_ignore_input.clone() else {
            return;
        };
        let text = if reset {
            settings::DEFAULT_ANALYZE_IGNORE.to_string()
        } else {
            input.read(cx).value().to_string()
        };
        match settings::write_analyze_ignore(&text) {
            Ok(()) => {
                self.settings_controls.saved_ignore = Some(text);
                self.settings_controls.ignore_error = None;
                if reset {
                    input.update(cx, |s, cx| {
                        s.set_value(settings::DEFAULT_ANALYZE_IGNORE, window, cx)
                    });
                }
                self.push_toast_for(
                    crate::ui::toast_stack::ToastSubject::Settings,
                    ToastKind::Info,
                    Msg::SettingsAnalyzeIgnoreSaved.t(),
                    cx,
                );
            }
            Err(error) => {
                let message = super::i18n::analyze_ignore_failed_fmt(&error.to_string());
                self.settings_controls.ignore_error = Some(message.clone());
                self.push_toast_for(
                    crate::ui::toast_stack::ToastSubject::Settings,
                    ToastKind::Error,
                    message,
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(super) fn analyze_ignore_dirty(&self, cx: &App) -> bool {
        self.analyze_ignore_input.as_ref().is_some_and(|input| {
            Some(input.read(cx).value().as_ref()) != self.settings_controls.saved_ignore.as_deref()
        })
    }
}

pub(super) fn smart_consent_alert(
    app: &Entity<KagiApp>,
    alert: SmartConsentAlert,
    cx: &App,
) -> impl IntoElement {
    use super::dialog_a11y::{apply_dialog, dialog_a11y, ConfirmStage, DialogHandler};
    use gpui_component::FocusTrapElement as _;
    let owner = app.downgrade();
    let confirm: DialogHandler = std::rc::Rc::new(move |window, cx| {
        let _ = owner.update(cx, |app, cx| {
            app.finish_settings_smart_consent(true, window, cx)
        });
    });
    let owner = app.downgrade();
    let cancel: DialogHandler = std::rc::Rc::new(move |window, cx| {
        let _ = owner.update(cx, |app, cx| {
            app.finish_settings_smart_consent(false, window, cx)
        });
    });
    let card = super::modal_renderers_misc::smart_consent_card(confirm.clone(), cancel.clone(), cx)
        .w(gpui::relative(0.9))
        .max_w(super::theme::scaled_px(super::modal_shell::MODAL_W_MD));
    let key_cancel = cancel.clone();
    let card = apply_dialog(
        "settings-smart-consent-alert",
        div().id("settings-smart-consent-alert"),
        dialog_a11y(
            Msg::SmartConsentTitle.t(),
            Some(Msg::SmartConsentEnable.t()),
            false,
            ConfirmStage::Single,
        ),
        Some(confirm),
        cancel.clone(),
    )
    .focus_trap("settings-smart-consent-alert", &alert.focus)
    .key_context("KagiCancelDialog")
    .on_action(move |_: &super::CloseMainDiff, window, cx| {
        cancel(window, cx);
        cx.stop_propagation();
    })
    .on_key_down(move |event, window, cx| {
        if matches!(event.keystroke.key.as_str(), "enter" | "escape") {
            key_cancel(window, cx);
            cx.stop_propagation();
        }
    })
    .child(card);
    super::modal_renderers::modal_overlay(super::e2e::measure_control(
        "settings-smart-consent-alert",
        card,
    ))
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn settings_choices_for_e2e(&self, models: bool) -> Vec<(gpui::Role, String, bool, bool)> {
        if models {
            &self.settings_controls.models
        } else {
            &self.settings_controls.provider
        }
        .recorded_tabs()
    }
    pub fn focus_settings_choice_for_e2e(
        &self,
        models: bool,
        slot: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        if models {
            &self.settings_controls.models
        } else {
            &self.settings_controls.provider
        }
        .focus(slot, window, cx);
    }
    pub fn analyze_ignore_dirty_for_e2e(&self, cx: &App) -> bool {
        self.analyze_ignore_dirty(cx)
    }
}
