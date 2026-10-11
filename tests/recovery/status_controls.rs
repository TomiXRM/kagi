//! #1207: named status buttons are reachable and activate exactly once.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use gpui::{px, size, Role, Toggled, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_status_controls(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    for appearance in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(appearance));
        for (language, labels) in [
            ("en", ["Terminal", "Operation Log"]),
            ("ja", ["ターミナル", "操作ログ"]),
        ] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                let captured = Rc::new(RefCell::new(None));
                let output = captured.clone();
                let state = e2e::app_state(fixture.path()).unwrap();
                let win = crate::macos::open_offscreen(
                    cx,
                    size(px(600.), px(660.)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                )
                .into();
                let app = captured.borrow().clone().unwrap();
                cx.run_until_parked();
                let draw = |cx: &mut VisualTestAppContext| {
                    cx.update_window(win, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                };
                draw(cx);
                cx.update_window(win, |_, window, cx| {
                    app.read(cx).root_focus.clone().unwrap().focus(window, cx)
                })
                .unwrap();
                for (id, label) in ["status-icon-terminal", "status-icon-oplog"]
                    .into_iter()
                    .zip(labels)
                {
                    assert_eq!(
                        e2e::toolbar_label(id),
                        Some(label),
                        "{language}: status control AX name"
                    );
                    let (role, name, _) = e2e::status_control(id).expect("rendered AX button");
                    assert_eq!((role, name), (Role::Button, label));
                    let bounds = e2e::control_bounds(win.window_id(), id).unwrap();
                    let footer = e2e::control_bounds(win.window_id(), "pr-menu-footer").unwrap();
                    contained(footer, bounds, id);
                    // measure_inside records the content box, inside the fixed 2px ring.
                    let expected = f32::from(theme::scaled_px(22.)) - 4.;
                    assert!(
                        (f32::from(bounds.size.height) - expected).abs() <= 0.5,
                        "{appearance}/{language}/{zoom}: {id} content height {:?}, expected {expected}",
                        bounds.size.height
                    );
                    let tip = e2e::toolbar_caption_and_tooltip(id).1.unwrap();
                    assert!(tip.contains(if id == "status-icon-terminal" {
                        "⌘J"
                    } else {
                        "↵ / Space"
                    }));
                }
                let mut reached = Vec::new();
                for _ in 0..120 {
                    cx.update_window(win, |_, window, cx| {
                        window.focus_next(cx);
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let _ = e2e::take_focus_probe();
                    cx.simulate_keystrokes(win, "f19");
                    if let Some(id @ ("status-icon-terminal" | "status-icon-oplog")) =
                        e2e::take_focus_probe()
                    {
                        reached.push(id);
                        let tab = if id == "status-icon-terminal" {
                            BottomTab::Terminal
                        } else {
                            BottomTab::OperationLog
                        };
                        for key in ["enter", "space"] {
                            let was_open = cx.read(|cx| {
                                app.read(cx).bottom_panel_open && app.read(cx).bottom_tab == tab
                            });
                            let before_tip = e2e::toolbar_caption_and_tooltip(id).1.unwrap();
                            crate::keyboard_nav::keys(cx, win, key);
                            cx.run_until_parked();
                            draw(cx);
                            assert_eq!(
                                cx.read(|cx| app.read(cx).bottom_panel_open
                                    && app.read(cx).bottom_tab == tab),
                                !was_open,
                                "{id}: {key} activates exactly once"
                            );
                            assert_eq!(
                                e2e::status_control(id).unwrap().2,
                                if was_open {
                                    Toggled::False
                                } else {
                                    Toggled::True
                                }
                            );
                            assert_ne!(
                                e2e::toolbar_caption_and_tooltip(id).1.unwrap(),
                                before_tip,
                                "show/hide tooltip flips with state"
                            );
                        }
                        if reached.len() == 2 {
                            break;
                        }
                    }
                }
                assert_eq!(
                    reached,
                    ["status-icon-terminal", "status-icon-oplog"],
                    "status buttons follow content in Tab order"
                );
                unmount(cx, app, win);
            }
        }
    }
}
