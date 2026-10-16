//! Clipboard approval and focus escape at the terminal's existing session boundary.
use crate::ui::{i18n::Msg, modals::TerminalPasteModal, KagiApp};
use gpui::{div, prelude::*, Context, SharedString, Window};

impl KagiApp {
    pub(crate) fn paste_into_terminal(&mut self, text: String, cx: &mut Context<Self>) {
        // A clipboard shortcut may not replace an unrelated confirmation.
        if self.has_active_modal() {
            return;
        }
        let Some(owner) = self.active_session() else {
            return;
        };
        let Some(session) = self
            .ui
            .get(&owner)
            .and_then(|ui| ui.terminal_session.as_ref())
        else {
            return;
        };
        let Some(view) = session.view.as_ref() else {
            return;
        };
        if view.read(cx).paste_text(&text, false) {
            klog!("terminal: paste {} chars", text.chars().count());
        } else {
            let generation = session.spawns;
            self.set_terminal_paste_modal(TerminalPasteModal {
                owner,
                generation,
                text: text.into(),
            });
            cx.notify();
        }
    }

    pub(crate) fn confirm_terminal_paste(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.terminal_paste_modal().cloned() else {
            return;
        };
        self.clear_terminal_paste_modal();
        // Never paste into a restarted shell, a switched tab, or a dead owner.
        let view = self
            .ui
            .get(&modal.owner)
            .filter(|_| self.active_session() == Some(modal.owner))
            .and_then(|ui| ui.terminal_session.as_ref())
            .filter(|session| session.spawns == modal.generation)
            .and_then(|session| session.view.as_ref());
        if let Some(view) = view {
            view.read(cx).paste_text(&modal.text, true);
            klog!("terminal: paste {} chars", modal.text.chars().count());
        }
        cx.notify();
    }

    pub(crate) fn exit_terminal_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bottom_panel_open && self.bottom_tab == crate::ui::BottomTab::Terminal {
            let handles = self
                .bottom_tab_focus
                .sync(&crate::ui::keyboard_nav::slot_keys(3), cx);
            handles[1].focus(window, cx);
        } else {
            self.yield_terminal_focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn yield_terminal_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .ui()
            .terminal_session
            .as_ref()
            .and_then(|session| session.view.as_ref())
            .is_some_and(|view| view.read(cx).focus_handle().is_focused(window));
        if let Some(root) = self.root_focus.as_ref().filter(|_| focused) {
            root.focus(window, cx);
        }
    }
}

pub(crate) fn render_paste_confirmation(
    modal: TerminalPasteModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use crate::ui::{
        button_style::{modal_button, modal_button_row, ModalButtonKind},
        modal_renderers::{modal_overlay, render_modal_title_row},
        modal_shell,
    };
    let cancel = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
        app.clear_terminal_paste_modal();
        cx.notify();
    });
    let confirm = cx.listener(|app, _: &gpui::ClickEvent, _, cx| app.confirm_terminal_paste(cx));
    let description = SharedString::from(
        if gpui_terminal::TerminalView::paste_contains_control_characters(&modal.text) {
            Msg::TerminalPasteControlBody
        } else {
            Msg::TerminalPasteBody
        }
        .t(),
    );
    #[cfg(feature = "gui-e2e")]
    PASTE_DESCRIPTION.with(|slot| *slot.borrow_mut() = Some(description.clone()));
    let body = modal_shell::modal_scroll_body()
        .child(render_modal_title_row(
            SharedString::from(Msg::TerminalPasteTitle.t()),
            None,
        ))
        .child(div().text_sm().child(description.clone()))
        .child(
            div()
                .text_sm()
                .font_family(crate::ui::MONO_FONT)
                .child(modal.text),
        );
    let card = modal_shell::modal_card(460.)
        .id("terminal-paste-dialog")
        .role(gpui::Role::AlertDialog)
        .aria_label(SharedString::from(Msg::TerminalPasteTitle.t()))
        .aria_description(description)
        .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
            builder.parent_node().set_modal()
        })
        .child(body)
        .child(
            modal_button_row()
                .child(crate::ui::e2e::measure_control(
                    "terminal-paste-cancel",
                    modal_button(
                        "terminal-paste-cancel",
                        Msg::PlanCancel.t(),
                        ModalButtonKind::Cancel,
                        None,
                        cancel,
                        cx,
                    ),
                ))
                .child(crate::ui::e2e::measure_control(
                    "terminal-paste-confirm",
                    modal_button(
                        "terminal-paste-confirm",
                        Msg::TerminalPasteConfirm.t(),
                        ModalButtonKind::Primary,
                        None,
                        confirm,
                        cx,
                    ),
                )),
        );
    modal_overlay(card).into_any_element()
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DESCRIPTION: std::cell::RefCell<Option<SharedString>> = const { std::cell::RefCell::new(None) };
    static PASTE_DESCRIPTION: std::cell::RefCell<Option<SharedString>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn focus_description() -> SharedString {
    let description = SharedString::from(Msg::TerminalFocusDescription.t());
    #[cfg(feature = "gui-e2e")]
    DESCRIPTION.with(|slot| *slot.borrow_mut() = Some(description.clone()));
    description
}
#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn terminal_description_for_e2e(&self) -> Option<SharedString> {
        DESCRIPTION.with(|slot| slot.borrow().clone())
    }
    pub fn terminal_paste_description_for_e2e(&self) -> Option<SharedString> {
        PASTE_DESCRIPTION.with(|slot| slot.borrow().clone())
    }
}
