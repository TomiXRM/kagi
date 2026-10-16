//! #1441: help content scrolls independently beneath a pinned title/dismiss row.
use crate::close_window_initial_home::{draw, mount};
use gpui::{px, AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{commands, e2e};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name).unwrap_or_else(|| panic!("{name} is laid out"))
}
fn inside(parent: Bounds<Pixels>, child: Bounds<Pixels>) -> bool {
    child.top() >= parent.top() - px(1.) && child.bottom() <= parent.bottom() + px(1.)
}

pub fn scenario_shortcuts_panel_scroll(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["ui_zoom", "lang"]);
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    assert!(
        commands::shortcut_listing().len() >= 28,
        "default registry supplies the full list"
    );
    let fixture = crate::macos::build_fixture();
    for viewport in [(700., 500.), (940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for lang in [Lang::En, Lang::Ja] {
                check_at(cx, fixture.path(), viewport, zoom, lang);
            }
        }
    }
    theme::set_zoom(old_zoom);
    i18n::set_lang(old_lang);
}

fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    cx.simulate_keystrokes(window, key);
    cx.run_until_parked();
    draw(cx, window);
}

fn check_at(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    (width, height): (f32, f32),
    zoom: f32,
    lang: Lang,
) {
    theme::set_zoom(zoom);
    i18n::set_lang(lang);
    let (app, window) = mount(cx, e2e::app_state(repo).unwrap(), width, height);
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |a, cx| a.handle_menu_command("help.shortcuts", w, cx));
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
    let scroll = app.read_with(cx, |a, _| match a.menu_overlay.as_ref().unwrap() {
        commands::MenuOverlay::Info { scroll, .. } => scroll.clone(),
        _ => panic!("shortcut help owns its body scroll"),
    });
    assert!(
        scroll.max_offset().y > px(0.),
        "help has a scrollable extent"
    );
    let panel = bounds(window, "shortcuts-panel");
    let header = bounds(window, "shortcuts-header");
    assert!(
        panel.top() >= px(0.) && panel.bottom() <= px(height),
        "{width}x{height} {zoom} {lang:?}: panel fits the viewport"
    );
    assert!(inside(panel, header), "header starts visible");
    let before = bounds(window, "shortcuts-tail");
    if (width, height) == (700., 500.) {
        assert!(
            !inside(panel, before),
            "full help content overflows the small card"
        );
    }
    keys(cx, window, "end");
    check_end(cx, window, &scroll, panel, header);
    keys(cx, window, "home");
    assert_eq!(
        bounds(window, "shortcuts-tail"),
        before,
        "Home returns to the body start"
    );
    keys(cx, window, "escape");
    assert!(app.read_with(cx, |a, _| a.menu_overlay.is_none()));
    crate::macos::unmount(cx, app, window);
}

fn check_end(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    scroll: &gpui::ScrollHandle,
    panel: Bounds<Pixels>,
    header: Bounds<Pixels>,
) {
    let tail = bounds(window, "shortcuts-tail");
    assert!(
        inside(panel, tail),
        "End reveals the final remap explanation: {tail:?} in {panel:?}"
    );
    assert_eq!(
        bounds(window, "shortcuts-header"),
        header,
        "title is pinned while body scrolls"
    );
    let dismiss = bounds(window, "shortcuts-dismiss");
    assert!(
        inside(header, dismiss),
        "dismiss remains visible in the pinned header"
    );
    let body = scroll.bounds();
    assert!(
        inside(body, tail),
        "final row is contained by its own scroll viewport"
    );
    let background = e2e::workspace_scrolls();
    for position in [body.center(), header.center()] {
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-10_000.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
    }
    assert_eq!(
        e2e::workspace_scrolls(),
        background,
        "help owns wheel input at its boundary and header"
    );
}
