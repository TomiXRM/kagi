//! Renderer-fed shortcut captions; no binding or display synthesis in the oracle.
use gpui::SharedString;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static SHORTCUTS: std::cell::RefCell<std::collections::BTreeMap<usize, SharedString>> =
        std::cell::RefCell::new(Default::default());
}

pub(crate) fn shortcut(index: usize, text: String) -> SharedString {
    let text = SharedString::from(text);
    #[cfg(feature = "gui-e2e")]
    SHORTCUTS.with(|values| values.borrow_mut().insert(index, text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = index;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_shortcut(index: usize) -> Option<SharedString> {
    SHORTCUTS.with(|values| values.borrow().get(&index).cloned())
}
