//! In-memory system display injection and production renderer observations.
use kagi_ui_core::accessibility::{set_system_display, system_display};
use std::cell::Cell;

thread_local! {
    static LOADING_ANIMATIONS: Cell<usize> = const { Cell::new(0) };
    static SPINNER_ANIMATIONS: Cell<usize> = const { Cell::new(0) };
}

pub struct SystemDisplayInjection((bool, bool));
impl SystemDisplayInjection {
    pub fn new(motion: bool, transparency: bool) -> Self {
        let before = system_display();
        set_system_display(motion, transparency);
        Self(before)
    }
    pub fn update(&self, motion: bool, transparency: bool, cx: &mut gpui::App) {
        set_system_display(motion, transparency);
        cx.refresh_windows();
    }
}
impl Drop for SystemDisplayInjection {
    fn drop(&mut self) {
        set_system_display(self.0 .0, self.0 .1);
    }
}

pub(crate) fn record_loading_animation(animated: bool) {
    if animated {
        LOADING_ANIMATIONS.with(|count| count.set(count.get() + 1));
    }
}

pub(crate) fn record_spinner_animation(animated: bool) {
    if animated {
        SPINNER_ANIMATIONS.with(|count| count.set(count.get() + 1));
    }
}

pub fn spinner_animation_count() -> usize {
    SPINNER_ANIMATIONS.with(|count| count.set(0));
    let _spinner = super::super::render_overlay::sync_spinner(12., 0xffffff, "a11y-spinner-probe");
    SPINNER_ANIMATIONS.with(Cell::get)
}
/// Build the actual loading element, counting only its with_animation branches.
pub fn loading_animation_count() -> usize {
    LOADING_ANIMATIONS.with(|count| count.set(0));
    let _dots = super::super::render_body::render_loading_dots();
    LOADING_ANIMATIONS.with(Cell::get)
}

pub fn dashboard_card_fill() -> gpui::Hsla {
    super::super::pr_attention::dash_card_bg()
}
