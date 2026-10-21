//! Presentation setup shared by plan-card scenarios; assertions stay with callers.
use crate::macos::{open_offscreen, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, path::Path, rc::Rc};

pub struct Presentation {
    language: i18n::Lang,
    zoom: f32,
    palette: String,
    // Drop restores the globals before this field restores persisted keys.
    _saved: crate::gui_isolation::SavedKeys,
}
impl Presentation {
    pub fn keep() -> Self {
        Self {
            language: i18n::lang(),
            zoom: theme::zoom(),
            palette: theme::theme().slug.to_string(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]),
        }
    }
}
impl Drop for Presentation {
    fn drop(&mut self) {
        i18n::set_lang(self.language);
        theme::set_zoom(self.zoom);
        theme::set_active(&self.palette);
    }
}

pub fn mount(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    dimensions: (f32, f32),
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let capture = Rc::new(RefCell::new(None));
    let out = capture.clone();
    let window = open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &out),
    );
    let app = capture.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window.into())
}

pub fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

pub fn each_presentation(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    mut check: impl FnMut(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    for dimensions in [(940., 660.), (1392., 883.)] {
        for language in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(language);
            for palette in ["apple-light", "apple-dark"] {
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    let (app, window) = mount(cx, repo, dimensions);
                    app.update(cx, |app, cx| app.set_theme(palette, cx));
                    check(cx, &app, window);
                    unmount(cx, app, window);
                }
            }
        }
    }
}
