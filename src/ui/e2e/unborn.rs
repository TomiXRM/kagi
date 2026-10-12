//! Renderer-fed copy and Button semantics, not native VoiceOver evidence.
use gpui::{Role, SharedString};
use std::cell::RefCell;

pub struct EmptyState {
    pub title: SharedString,
    pub hint: SharedString,
    pub role: Role,
    pub name: &'static str,
}

thread_local! {
    static EMPTY_STATE: RefCell<Option<EmptyState>> = const { RefCell::new(None) };
    static BUTTON_FOCUS: RefCell<Option<gpui::FocusHandle>> = const { RefCell::new(None) };
}

pub(crate) fn record(title: &SharedString, hint: &SharedString, name: &'static str) {
    EMPTY_STATE.with(|slot| {
        *slot.borrow_mut() = Some(EmptyState {
            title: title.clone(),
            hint: hint.clone(),
            role: Role::Button,
            name,
        });
    });
}

pub fn take() -> Option<EmptyState> {
    EMPTY_STATE.with(|slot| slot.borrow_mut().take())
}

pub fn take_focus() -> gpui::FocusHandle {
    BUTTON_FOCUS.with(|slot| slot.borrow_mut().take().expect("Changes Button rendered"))
}

/// Observe the pinned Button's own focus owner only during its layout pass.
#[derive(gpui::IntoElement)]
pub(crate) struct ChangesButton(pub gpui_component::button::Button);

impl gpui::RenderOnce for ChangesButton {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        let focus = window
            .use_keyed_state("unborn-open-changes", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        BUTTON_FOCUS.with(|slot| *slot.borrow_mut() = Some(focus));
        self.0.render(window, cx)
    }
}
