//! Analyze draft reset confirmation shared by both Settings hosts.
use super::{i18n::Msg, KagiApp};
use gpui::{div, prelude::*, App, Context, Entity, FocusHandle, Window, WindowId};
use gpui_component::{button::Button, FocusTrapElement as _};

#[derive(Clone)]
pub(super) struct AnalyzeResetAlert {
    pub window: WindowId,
    pub focus: FocusHandle,
    pub alert_return_focus: super::overlay_focus::AlertReturnFocus,
}

impl KagiApp {
    pub(super) fn analyze_reset_alert(&self) -> Option<&AnalyzeResetAlert> {
        self.settings_controls.reset_alert.as_ref()
    }

    pub(super) fn clear_analyze_reset_alert(&mut self, window: WindowId) -> bool {
        if self
            .analyze_reset_alert()
            .is_some_and(|alert| alert.window == window)
        {
            self.settings_controls.reset_alert = None;
            self.settings_controls.reset_request = None;
            true
        } else {
            false
        }
    }

    pub(super) fn request_analyze_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.analyze_ignore_dirty(cx) {
            self.save_analyze_ignore(true, window, cx);
            return;
        }
        let focus = cx.focus_handle();
        self.settings_controls.reset_alert = Some(AnalyzeResetAlert {
            window: window.window_handle().window_id(),
            alert_return_focus: super::overlay_focus::AlertReturnFocus::for_settings(
                window,
                cx,
                focus.clone(),
            ),
            focus: focus.clone(),
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn finish_analyze_reset(
        &mut self,
        confirm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self
            .settings_controls
            .reset_alert
            .take_if(|alert| alert.window == window.window_handle().window_id())
        else {
            return;
        };
        modal.alert_return_focus.restore_settings(window, cx);
        if confirm {
            self.save_analyze_ignore(true, window, cx);
        } else {
            self.settings_controls.reset_request = None;
        }
        cx.notify();
    }
}

pub(super) fn reset_alert(
    app: &Entity<KagiApp>,
    modal: AnalyzeResetAlert,
    cx: &App,
) -> impl IntoElement {
    use super::dialog_a11y::{apply_dialog, dialog_a11y, ConfirmStage, DialogHandler};
    let owner = app.downgrade();
    let confirm: DialogHandler = std::rc::Rc::new(move |window, cx| {
        let _ = owner.update(cx, |app, cx| app.finish_analyze_reset(true, window, cx));
    });
    let owner = app.downgrade();
    let cancel: DialogHandler = std::rc::Rc::new(move |window, cx| {
        let _ = owner.update(cx, |app, cx| app.finish_analyze_reset(false, window, cx));
    });
    let key_cancel = cancel.clone();
    let action_cancel = cancel.clone();
    let cancel_click = cancel.clone();
    let confirm_click = confirm.clone();
    let title = Msg::SettingsAnalyzeResetTitle.t();
    let card = super::modal_shell::modal_card_sized()
        .w(gpui::relative(0.9))
        .max_w(gpui::px(super::modal_shell::MODAL_W_SM))
        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
        .child(super::modal_shell::modal_body().child(Msg::SettingsAnalyzeResetDetail.t()))
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(super::e2e::measure_control(
                    "settings-analyze-reset-cancel",
                    Button::new("settings-reset-cancel")
                        .label(Msg::PlanCancel.t())
                        .outline()
                        .on_click(move |_, window, cx| cancel_click(window, cx)),
                ))
                .child(super::e2e::measure_control(
                    "settings-analyze-reset-confirm",
                    super::button_style::modal_button(
                        "settings-reset-confirm",
                        Msg::SettingsAnalyzeIgnoreReset.t(),
                        super::button_style::ModalButtonKind::Destructive,
                        None,
                        move |_, window, cx| confirm_click(window, cx),
                        cx,
                    ),
                )),
        );
    let card = apply_dialog(
        "settings-analyze-reset-alert",
        div().id("settings-analyze-reset-alert"),
        dialog_a11y(
            title,
            Some(Msg::SettingsAnalyzeIgnoreReset.t()),
            true,
            ConfirmStage::Single,
        ),
        Some(confirm),
        cancel,
    )
    .focus_trap("settings-analyze-reset-alert", &modal.focus)
    .key_context("KagiCancelDialog")
    .on_action(move |_: &super::CloseMainDiff, window, cx| {
        action_cancel(window, cx);
        cx.stop_propagation();
    })
    .on_key_down(
        move |event, window, cx| match event.keystroke.key.as_str() {
            "enter" => cx.stop_propagation(),
            "escape" => {
                key_cancel(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        },
    )
    .child(card);
    super::modal_renderers::modal_overlay(super::e2e::measure_control(
        "settings-analyze-reset-alert",
        card,
    ))
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn analyze_reset_focus_for_e2e(&self, window: &Window, cx: &App) -> Option<bool> {
        self.analyze_reset_alert()
            .filter(|alert| alert.window == window.window_handle().window_id())
            .map(|modal| modal.focus.contains_focused(window, cx))
    }
}
