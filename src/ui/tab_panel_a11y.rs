//! The content a tab list switches, for assistive technology (#979).
//!
//! A `keyboard_nav::TabList` names its cells `Role::Tab`; the area that
//! shows the selected tab's content is a `Role::TabPanel` named after that
//! tab. gpui has no `aria-labelledby` / `aria-controls`, so the panel carries
//! the selected tab's name as its own label. Tier A cannot read the AX tree
//! and reads what the renderer set, recorded here (`list_a11y`'s pattern).

use gpui::{prelude::*, Div, Role, SharedString, Stateful};

/// Mark `el` (the panel `id`) as the content of the selected tab `label`.
pub(crate) fn tab_panel(
    el: Stateful<Div>,
    id: &'static str,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    let (role, label) = (Role::TabPanel, label.into());
    #[cfg(feature = "gui-e2e")]
    record(id, role, &label);
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    el.role(role).aria_label(label)
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static PANELS: std::cell::RefCell<std::collections::HashMap<&'static str, (Role, String)>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
fn record(id: &'static str, role: Role, label: &str) {
    PANELS.with(|m| m.borrow_mut().insert(id, (role, label.to_string())));
}

/// The role and label the drawn frames gave panel `id`, if any did since
/// the last [`clear_recorded_tab_panels`].
#[cfg(feature = "gui-e2e")]
pub fn recorded_tab_panel(id: &str) -> Option<(Role, String)> {
    PANELS.with(|m| m.borrow().get(id).cloned())
}

/// Forget recorded panels, so the next frame proves which it draws.
#[cfg(feature = "gui-e2e")]
pub fn clear_recorded_tab_panels() {
    PANELS.with(|m| m.borrow_mut().clear());
}
