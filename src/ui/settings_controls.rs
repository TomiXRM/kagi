//! Native Settings consent and Analyze persistence effects.
use super::{i18n::Msg, settings, KagiApp, ToastKind};
use gpui::{div, prelude::*, App, Context, Entity, Window};

#[derive(Default)]
pub(crate) struct SettingsControls {
    pub view: Option<Entity<kagi_ui_settings::SettingsView>>,
    pub saved_ignore: Option<String>,
    pub reset_request: Option<kagi_ui_settings::AnalyzeRequest>,
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
        self.seed_settings(cx);
        cx.notify();
    }

    pub(super) fn save_analyze_ignore(
        &mut self,
        reset: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.settings_controls.view.clone() else { return };
        let request = self.settings_controls.reset_request.take()
            .unwrap_or_else(|| view.read(cx).analyze_request(cx));
        self.save_analyze_request(request, reset, window, cx);
    }

    pub(super) fn save_analyze_request(
        &mut self,
        request: kagi_ui_settings::AnalyzeRequest,
        reset: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = if reset { settings::DEFAULT_ANALYZE_IGNORE } else { request.text.as_ref() };
        let result = match settings::write_analyze_ignore(text) {
            Ok(()) => {
                self.settings_controls.saved_ignore = Some(text.to_owned());
                self.push_toast_for(super::toast_stack::ToastSubject::Settings, ToastKind::Info, Msg::SettingsAnalyzeIgnoreSaved.t(), cx);
                if reset {
                    kagi_ui_settings::AnalyzeResult::Reset { request, text: settings::DEFAULT_ANALYZE_IGNORE.into() }
                } else {
                    kagi_ui_settings::AnalyzeResult::Saved(request)
                }
            }
            Err(error) => {
                let message = super::i18n::analyze_ignore_failed_fmt(&error.to_string());
                self.push_toast_for(super::toast_stack::ToastSubject::Settings, ToastKind::Error, message.clone(), cx);
                kagi_ui_settings::AnalyzeResult::Failed { request, error: message.into() }
            }
        };
        if let Some(view) = &self.settings_controls.view {
            view.update(cx, |view, cx| view.seed_analyze_result(result, window, cx));
        }
        self.seed_settings(cx);
        cx.notify();
    }

    pub(super) fn analyze_ignore_dirty(&self, cx: &App) -> bool {
        self.settings_controls.view.as_ref().is_some_and(|view| view.read(cx).analyze_dirty(cx))
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
    pub fn analyze_ignore_dirty_for_e2e(&self, cx: &App) -> bool {
        self.analyze_ignore_dirty(cx)
    }
}
