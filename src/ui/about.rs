//! Informational About task in the shared named-dialog slot.
use super::button_style::{modal_button, modal_button_row, ModalButtonKind};
use super::modal_renderers::{input_dialog_frame, modal_overlay};
use super::*;

pub struct AboutModal {
    pub opener: Option<FocusHandle>,
}
impl KagiApp {
    pub(super) fn open_about_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.about_modal().is_some() {
            return;
        }
        self.leave_settings_for_overlay();
        let opener = window.focused(cx);
        self.menu_overlay = None;
        self.modal_focus = Some(cx.focus_handle());
        self.set_about_modal(AboutModal { opener });
    }
}
pub(super) fn render_about(
    focus: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    if let Some(focus) = &focus {
        if !focus.is_focused(window) && !focus.contains_focused(window, cx) {
            window.focus(focus, cx);
        }
    }
    let close = modal_button(
        "about-close",
        Msg::RemoteClose.t(),
        ModalButtonKind::Cancel,
        None,
        cx.listener(|app, _, _, cx| {
            app.clear_about_modal();
            cx.notify();
        }),
        cx,
    );
    let information = gpui_component::text::TextView::html(
        "about-information",
        SharedString::from(format!(
            "<p>Kagi {}</p><p>© 2026 TomiXRM</p><p><a href=\"{}\">{}</a></p>",
            env!("CARGO_PKG_VERSION"),
            super::commands::GITHUB_URL,
            super::commands::GITHUB_URL
        )),
    )
    .selectable(true);
    let card = super::modal_shell::modal_card(super::modal_shell::MODAL_W_MD)
        .occlude()
        .child(
            div()
                .flex_shrink_0()
                .text_size(theme::scaled_px(16.))
                .text_color(rgb(theme().text_main))
                .child(Msg::AboutTitle.t()),
        )
        .child(
            super::modal_shell::modal_scroll_body()
                .text_color(rgb(theme().text_sub))
                .child(Msg::AboutDescription.t())
                .child(information),
        )
        .child(modal_button_row().child(super::e2e::measure_control("about-close", close)));
    let frame = input_dialog_frame(
        "about-card",
        Msg::AboutTitle.t(),
        None,
        card,
        focus.as_ref(),
        |frame| {
            frame
                .relative()
                .child(super::e2e::measure_inside("about-card"))
        },
        cx,
    );
    modal_overlay(frame).into_any_element()
}
