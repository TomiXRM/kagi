//! Settings' appearance theme picker. The gpui-component `Select` is an entity
//! that needs a `Window`, so it cannot be built in `KagiApp::new`; it is built
//! with the rest of the window-bound entity state in `e2e::build_kagi_entity`,
//! which the real window and the offscreen GUI E2E mount share (ADR-0166).

use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::select::SelectEvent;

use super::settings_view::{self, ThemeOption, ThemeSelectState};
use super::KagiApp;

/// Build the picker and apply + persist a confirmed theme via `set_theme`.
pub(super) fn install(kagi: &Entity<KagiApp>, window: &mut Window, cx: &mut App) {
    let theme_select = cx.new(|cx| {
        ThemeSelectState::new(
            settings_view::theme_options(),
            Some(settings_view::current_theme_index()),
            window,
            cx,
        )
    });
    kagi.update(cx, |app, cx| {
        cx.subscribe(
            &theme_select,
            |this, _state, event: &SelectEvent<Vec<ThemeOption>>, cx| {
                if let SelectEvent::Confirm(Some(slug)) = event {
                    this.set_theme(slug, cx);
                    cx.notify();
                }
            },
        )
        .detach();
        app.theme_select = Some(theme_select);
    });
}
