//! Toolbar button state → (renders bright, AccessKit disabled) (#354, PR #797).
//!
//! Why a toolbar button renders bright or muted is one of two things, and
//! only one of them is a disabled control:
//! - `Availability(false)`: the action cannot run right now (Pull with no
//!   upstream, Pop with no stash, Undo with an empty history). The button
//!   keeps its click handler — pressing it explains why in the footer — but
//!   assistive technology is told it is disabled.
//! - `Selection(false)`: an equally usable toggle is merely off (the Terminal
//!   button still opens the panel when pressed). Muted, never disabled.
//!
//! The mapping is pure so the bridge's input can be pinned by a unit test;
//! `render_header::make_btn` feeds the second bool to
//! `A11ySubtreeBuilder::parent_node().set_disabled()`.

/// Why a toolbar button renders bright or muted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonState {
    Availability(bool),
    Selection(bool),
}

/// `(enabled, unavailable)`: `enabled` drives the palette and cursor,
/// `unavailable` the AccessKit `disabled` flag.
pub(crate) fn button_flags(state: ButtonState) -> (bool, bool) {
    match state {
        ButtonState::Availability(on) => (on, !on),
        ButtonState::Selection(on) => (on, false),
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    /// Per toolbar button id, the `unavailable` bool that gated the AccessKit
    /// `disabled` flag on the last frame that drew it.
    static UNAVAILABLE: std::cell::RefCell<std::collections::HashMap<&'static str, bool>> =
        std::cell::RefCell::new(Default::default());
}

/// Record the bool `make_btn` feeds to `set_disabled` (E2E oracle).
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_unavailable(id: &'static str, unavailable: bool) {
    UNAVAILABLE.with(|map| map.borrow_mut().insert(id, unavailable));
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
pub(crate) fn record_unavailable(_id: &'static str, _unavailable: bool) {}

/// Whether the toolbar button `id` was drawn disabled for assistive technology
/// on its last frame; `None` if it was never drawn.
#[cfg(feature = "gui-e2e")]
pub fn toolbar_unavailable(id: &str) -> Option<bool> {
    UNAVAILABLE.with(|map| map.borrow().get(id).copied())
}

/// Forget every recorded state so the next draw proves the current frame.
#[cfg(feature = "gui-e2e")]
pub fn clear_toolbar_unavailable() {
    UNAVAILABLE.with(|map| map.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_false_is_the_only_disabled_state() {
        assert_eq!(button_flags(ButtonState::Availability(true)), (true, false));
        assert_eq!(
            button_flags(ButtonState::Availability(false)),
            (false, true)
        );
        // A closed Terminal is muted but still opens on press: never disabled.
        assert_eq!(button_flags(ButtonState::Selection(false)), (false, false));
        assert_eq!(button_flags(ButtonState::Selection(true)), (true, false));
    }
}
