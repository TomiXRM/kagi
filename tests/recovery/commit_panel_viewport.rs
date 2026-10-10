//! #1167: the commit composer must not displace its primary actions.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{
    point, px, size, Focusable, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestAppContext,
};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_commit_panel_viewport(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    for (width, height, zoom) in [(1392., 883., 1.), (940., 660., 1.67)] {
        for locale in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            theme::set_zoom(zoom);
            let state = e2e::app_state(fixture.path()).unwrap();
            let captured = Rc::new(RefCell::new(None));
            let output = captured.clone();
            let window =
                crate::macos::open_offscreen(cx, size(px(width), px(height)), move |window, cx| {
                    e2e::mount_root(state, window, cx, &output)
                });
            let app = captured.borrow().clone().unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                let panel = app.read(cx).ui().commit_panel.clone().unwrap();
                let body = panel.read(cx).body_input.clone().unwrap();
                body.update(cx, |input, cx| {
                    input.set_value("body line\n".repeat(30), window, cx)
                });
            })
            .unwrap();
            for _ in 0..4 {
                cx.run_until_parked();
                draw(cx, window.into(), (width, height));
            }
            for name in [
                "cp-files",
                "cp-body",
                "cp-smart-suggest",
                "cp-commit-action",
            ] {
                let bounds = e2e::control_bounds(window.window_id(), name)
                    .unwrap_or_else(|| panic!("missing {name}"));
                eprintln!(
                    "commit-panel snapshot {locale}/{width}x{height}@{zoom} {name}: {bounds:?}"
                );
                assert!(
                    bounds.size.height > px(0.),
                    "{name} must remain usable: {bounds:?}"
                );
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= px(height),
                    "{locale}/{width}x{height}@{zoom}: {name} outside viewport: {bounds:?}"
                );
                assert!(
                    bounds.left() >= px(0.) && bounds.right() <= px(width),
                    "{name} outside viewport width: {bounds:?}"
                );
                if zoom == 1. {
                    let expected = match name {
                        "cp-files" => (1068., 110.5, 324., 411.5),
                        "cp-body" => (1080., 566., 300., 257.),
                        "cp-smart-suggest" => (1305., 798., 21.5, 21.5),
                        "cp-commit-action" => (1079., 831., 302., 25.),
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        bounds,
                        gpui::Bounds::new(
                            point(px(expected.0), px(expected.1)),
                            size(px(expected.2), px(expected.3))
                        ),
                        "1392×883@100% must retain its measured pre-fix bounds"
                    );
                }
            }
            let panel = cx
                .read(|cx| app.read(cx).ui().commit_panel.clone())
                .unwrap();
            let body = cx.read(|cx| panel.read(cx).body_input.clone()).unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                body.update(cx, |input, cx| {
                    input.set_cursor_position(
                        gpui_component::input::Position::default(),
                        window,
                        cx,
                    );
                    input.set_scroll_offset(point(px(0.), px(0.)), cx);
                });
            })
            .unwrap();
            draw(cx, window.into(), (width, height));
            let before = cx.read(|cx| body.read(cx).scroll_offset().y);
            let viewport = e2e::control_bounds(window.window_id(), "cp-body").unwrap();
            cx.simulate_event(
                window.into(),
                ScrollWheelEvent {
                    position: point(viewport.center().x, viewport.top() + px(24.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
                    touch_phase: TouchPhase::Moved,
                    ..Default::default()
                },
            );
            draw(cx, window.into(), (width, height));
            assert!(
                cx.read(|cx| body.read(cx).scroll_offset().y) < before,
                "wheel must scroll the body internally"
            );
            cx.update_window(window.into(), |_, window, cx| {
                window.focus(&body.read(cx).focus_handle(cx), cx);
            })
            .unwrap();
            cx.simulate_keystrokes(window.into(), "cmd-end");
            cx.simulate_input(window.into(), "editable tail");
            draw(cx, window.into(), (width, height));
            assert!(
                cx.read(|cx| body.read(cx).value().contains("editable tail")),
                "the bounded editor remains editable"
            );
            drop(body);
            drop(panel);
            unmount(cx, app, window.into());
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_viewport");
}
