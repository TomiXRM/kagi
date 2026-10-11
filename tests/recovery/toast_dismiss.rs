//! #1240: real failed-fetch notification reading and keyboard dismissal.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, ToastKind};
use std::time::{Duration, Instant};

pub fn scenario_toast_dismiss(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let old_zoom = theme::zoom();
    let old_lang = kagi::ui::i18n::lang();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            repo.join("missing").to_str().unwrap(),
        ],
    );
    let (app, window) = mount(cx, repo);
    crate::surface_scrollbars::resize(cx, window);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            theme::set_zoom(1.67);
            app.fetch_async(false, cx);
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
            cx.run_until_parked();
            assert!(Instant::now() < deadline, "failed fetch settles");
            std::thread::sleep(Duration::from_millis(2));
        }
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            });
            window.draw(cx).clear();
        })
        .unwrap();
        cx.read(|cx| assert!(app.read(cx).menu_overlay.is_some(), "Settings owns Escape"));
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(
                app.read(cx).menu_overlay.is_none(),
                "Escape closes Settings first"
            );
            assert!(
                stack.read(cx).toasts().last().unwrap().dismissing.is_none(),
                "menu cancellation preserves notification"
            );
        });
        hovered_error(cx, &stack, window);
        three_line_error(cx, &stack, window);
        drop(stack);
    }
    theme::set_zoom(old_zoom);
    kagi::ui::i18n::set_lang(old_lang);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS toast_dismiss: EN/JA hovered error survives eleven seconds and Escape dismisses");
}

fn hovered_error(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
) {
    let id = cx.read(|cx| {
        let toast = stack.read(cx).toasts().last().unwrap();
        assert_eq!(toast.kind, ToastKind::Error);
        toast.id
    });
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}"))
        .expect("close Button painted");
    assert!(close.origin.x >= gpui::px(0.) && close.origin.y >= gpui::px(0.));
    assert!(
        close.right() <= gpui::px(940.) && close.bottom() <= gpui::px(660.),
        "close Button inside window"
    );
    assert!(close.size.width >= gpui::px(20.) && close.size.height >= gpui::px(20.));
    cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(11))
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .any(|t| t.id == id && t.dismissing.is_none()),
            "hovered error remains after old eight-second deadline"
        )
    });
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .all(|t| t.id != id || t.dismissing.is_some()),
            "Escape closes newest toast"
        )
    });
}

fn three_line_error(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
) {
    stack.update(cx, |stack, cx| {
        stack.push_notify(
            ToastKind::Error,
            "First error line\nSecond error line\nThird error line",
            cx,
        );
    });
    let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        window.draw(cx).clear();
    })
    .unwrap();
    let message = e2e::control_bounds(window.window_id(), &format!("toast-message-{id}"))
        .expect("three-line error painted");
    let line_height = cx
        .update_window(window, |_, window, _| (window.rem_size() * 1.4).round())
        .unwrap();
    assert!(
        message.size.height >= line_height * 3. - gpui::px(1.),
        "all three error lines have painted height"
    );
    assert!(
        message.size.height <= line_height * 3. + gpui::px(1.),
        "error preview bounded to three lines"
    );
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
}
