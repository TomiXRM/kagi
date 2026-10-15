//! Kagi's text-editing menu, independent of the component's limited locales.
use gpui::{App, Entity, Menu, MenuItem, Window};
use gpui_component::input::{Copy, Cut, InputState, Paste, SelectAll};
use gpui_component::native_menu::NativeMenu;

use crate::i18n::Msg;

/// Fully replace the component menu, including its code-editor-only LSP items.
/// Resolve labels on opening so existing fields follow a language change.
pub fn context_menu(
    state: &Entity<InputState>,
    disabled: bool,
) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
    let state = state.downgrade();
    move |_, window, cx| {
        let state = state.clone();
        let position = window.mouse_position();
        // The component invokes the builder while InputState is leased. Read
        // the post-right-click selection only after that lease is released;
        // returning an empty menu suppresses the component's immediate popup.
        window.defer(cx, move |window, cx| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let selected = !state.read(cx).selected_range().is_empty();
            let menu = menu_model(selected, !disabled, cx.read_from_clipboard().is_some());
            #[cfg(feature = "gui-e2e")]
            if CAPTURE_NEXT_MENU.with(|armed| armed.replace(false)) {
                LAST_MENU.with(|record| *record.borrow_mut() = Some(menu));
                return;
            }
            NativeMenu::from(menu).show(position, window, cx);
        });
        NativeMenu::new()
    }
}

fn menu_model(selected: bool, editable: bool, clipboard: bool) -> Menu {
    let mut cut = MenuItem::action(Msg::MenuCut.t(), Cut);
    let mut copy = MenuItem::action(Msg::MenuCopy.t(), Copy);
    let mut paste = MenuItem::action(Msg::MenuPaste.t(), Paste);
    for (item, enabled) in [
        (&mut cut, selected && editable),
        (&mut copy, selected),
        (&mut paste, clipboard && editable),
    ] {
        if let MenuItem::Action { disabled, .. } = item {
            *disabled = !enabled;
        }
    }
    Menu {
        name: "".into(),
        disabled: false,
        items: vec![
            cut,
            copy,
            paste,
            MenuItem::Separator,
            MenuItem::action(Msg::MenuSelectAll.t(), SelectAll),
        ],
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static LAST_MENU: std::cell::RefCell<Option<Menu>> = const { std::cell::RefCell::new(None) };
    static CAPTURE_NEXT_MENU: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Intercept the next production popup at the native-renderer boundary.
#[cfg(feature = "gui-e2e")]
pub fn capture_next_menu_for_e2e() {
    LAST_MENU.with(|record| record.borrow_mut().take());
    CAPTURE_NEXT_MENU.with(|armed| armed.set(true));
}

/// The actual menu model most recently handed to the native popup renderer.
#[cfg(feature = "gui-e2e")]
pub fn take_menu_for_e2e() -> Option<Menu> {
    LAST_MENU.with(|record| record.borrow_mut().take())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Action;

    #[test]
    fn text_menu_availability_preserves_selection_clipboard_and_disabled_rules() {
        for selected in [false, true] {
            for editable in [false, true] {
                for clipboard in [false, true] {
                    let menu = menu_model(selected, editable, clipboard);
                    let commands: Vec<_> = menu
                        .items
                        .iter()
                        .filter_map(|item| match item {
                            MenuItem::Action {
                                action, disabled, ..
                            } => Some((action.name(), *disabled)),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(
                        commands,
                        [
                            (Cut.name(), !(selected && editable)),
                            (Copy.name(), !selected),
                            (Paste.name(), !(clipboard && editable)),
                            (SelectAll.name(), false),
                        ]
                    );
                    assert_eq!(menu.items.len(), 5);
                    assert!(matches!(menu.items[3], MenuItem::Separator));
                }
            }
        }
    }
}
