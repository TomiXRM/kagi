//! #1202: real mouse thumb drags on the production Settings and Home owners.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{point, px, size, AnyWindowHandle, Modifiers, MouseButton, VisualTestAppContext};
use gpui_component::scroll::{ScrollbarHandle, ScrollbarShow};
use kagi::ui::{e2e, theme};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, w, cx| w.draw(cx).clear())
        .unwrap();
}

pub(crate) fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    let wanted = size(px(940.), px(660.));
    cx.update_window(win, |_, w, _| w.resize(wanted)).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx.update_window(win, |_, w, _| w.viewport_size()).unwrap() == wanted {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native resize reaches 940×660"
        );
    }
}

pub(crate) fn drag<H: ScrollbarHandle>(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
    handle: &H,
) {
    draw(cx, win);
    let bounds =
        e2e::control_bounds(win.window_id(), id).expect("production scrollbar region drawn");
    let height = bounds.size.height;
    let extent = handle.content_size().height;
    assert!(extent > height + px(20.), "content must overflow");
    let thumb = (height / extent * height).max(px(48.));
    let from = point(bounds.right() - px(8.), bounds.top() + thumb / 2.);
    let to = point(from.x, bounds.bottom() - thumb / 2. - px(2.));
    let before = handle.offset();
    cx.simulate_mouse_move(win, from, None, Modifiers::none());
    draw(cx, win);
    cx.simulate_mouse_down(win, from, MouseButton::Left, Modifiers::none());
    draw(cx, win);
    for n in 1..=5 {
        cx.simulate_mouse_move(
            win,
            from + (to - from) * (n as f32 / 5.),
            MouseButton::Left,
            Modifiers::none(),
        );
        draw(cx, win);
    }
    cx.simulate_mouse_up(win, to, MouseButton::Left, Modifiers::none());
    draw(cx, win);
    eprintln!("[gui-e2e] {id}: region={bounds:?} extent={extent:?} thumb-height={thumb:?} offset-before={before:?} offset-after={:?}", handle.offset());
    assert!(
        handle.offset().y < before.y - px(20.),
        "thumb drag must move the real scroll owner"
    );
    let after = handle.offset();
    draw(cx, win);
    assert!(
        (handle.offset().y - after.y).abs() < px(1.),
        "redraw preserves scroll position"
    );
}

pub fn scenario_settings_scrollbar_drag(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "lang"]);
    let old_policy = cx.read(|cx| gpui_component::Theme::global(cx).scrollbar_show);
    let fixture = build_fixture();
    let (app, win) = mount(cx, fixture.path());
    assert!(theme::set_active("apple-light"));
    theme::set_zoom(1.);
    cx.update(theme::sync_gpui_component_theme);
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |app, cx| app.handle_menu_command("app.settings", w, cx));
    })
    .unwrap();
    resize(cx, win);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for policy in [ScrollbarShow::Always, ScrollbarShow::Scrolling] {
                cx.update(|cx| gpui_component::Theme::global_mut(cx).scrollbar_show = policy);
                let handle = cx.read(|cx| app.read(cx).settings_scroll.clone());
                handle.set_offset(point(px(0.), px(0.)));
                // Start auto-hide's visibility interval through the actual owner.
                draw(cx, win);
                if policy == ScrollbarShow::Scrolling {
                    handle.set_offset(point(px(0.), px(-1.)));
                    draw(cx, win);
                }
                drag(cx, win, "settings-scrollbar", &handle);
            }
        }
    }
    // Settings' existing Switch springs retain bool entities until their
    // animation callbacks finish (same teardown as settings_switches).
    for _ in 0..10 {
        cx.advance_clock(std::time::Duration::from_millis(100));
        draw(cx, win);
        cx.run_until_parked();
    }
    cx.update(|cx| gpui_component::Theme::global_mut(cx).scrollbar_show = old_policy);
    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
    unmount(cx, app, win);
}
