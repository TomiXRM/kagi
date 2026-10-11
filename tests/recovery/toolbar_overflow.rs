//! #1210: narrow, zoomed toolbars expose clipped commands through overflow.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::{contained, GlobalSettings};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_toolbar_overflow(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _settings = crate::gui_isolation::SavedKeys::keep(&["toolbar_display", "theme"]);
    let fixture = build_fixture();
    assert!(theme::set_active("apple-light"));
    for language in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", language);
        i18n::init_lang();
        for display in [
            settings::ToolbarDisplay::IconOnly,
            settings::ToolbarDisplay::IconAndText,
        ] {
            settings::set_toolbar_display(display);
            for (width, zoom) in [(940., 1670), (1392., 1000)] {
                theme::set_zoom(zoom as f32 / 1000.);
                let state = e2e::app_state(fixture.path()).unwrap();
                let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
                let output = captured.clone();
                let win: AnyWindowHandle = crate::macos::open_offscreen(
                    cx,
                    size(px(width), px(660.)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                )
                .into();
                let app = captured.borrow().clone().unwrap();
                cx.run_until_parked();
                app.update(cx, |app, cx| {
                    app.view_mut().status_summary.repo_name =
                        "長いリポジトリ名-very-long-repository-name".repeat(6);
                    app.view_mut().status_summary.branch = "feature/非常に長いブランチ名".repeat(6);
                    cx.notify();
                });
                for _ in 0..4 {
                    cx.update_window(win, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
                let overflow = e2e::control_bounds(win.window_id(), "tb-overflow");
                assert_eq!(
                    overflow.is_some(),
                    zoom == 1670,
                    "{language}/{display:?}/{width}: overflow visibility"
                );
                let viewport =
                    gpui::Bounds::new(gpui::point(px(0.), px(0.)), size(px(width), px(660.)));
                const IDS: [&str; 13] = [
                    "tb-pull",
                    "tb-push",
                    "tb-branch",
                    "tb-stash",
                    "tb-pop",
                    "tb-undo",
                    "tb-redo",
                    "tb-terminal",
                    "tb-graph-mode",
                    "tb-pr-mode",
                    "tb-editor-ws",
                    "tb-ecosystem",
                    "tb-settings",
                ];
                for id in IDS {
                    e2e::clear_control_bounds(win.window_id(), id);
                }
                cx.update_window(win, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })
                .unwrap();
                let visible: [bool; 13] = std::array::from_fn(|i| {
                    let bounds = e2e::control_bounds(win.window_id(), IDS[i])
                        .filter(|b| b.size.width > px(0.));
                    if let Some(bounds) = bounds {
                        contained(viewport, bounds, IDS[i]);
                    }
                    bounds.is_some()
                });
                if let Some(bounds) = overflow {
                    contained(viewport, bounds, "overflow within window");
                    cx.update_window(win, |_, window, cx| {
                        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
                    })
                    .unwrap();
                    let mut reached = false;
                    for _ in 0..120 {
                        cx.update_window(win, |_, window, cx| {
                            window.focus_next(cx);
                            window.draw(cx).clear();
                        })
                        .unwrap();
                        let _ = e2e::take_focus_probe();
                        cx.simulate_keystrokes(win, "f19");
                        let focus = e2e::take_focus_probe();
                        if let Some(index) =
                            focus.and_then(|id| IDS.iter().position(|candidate| *candidate == id))
                        {
                            assert!(
                                visible[index],
                                "hidden command {} cannot remain a Tab stop",
                                IDS[index]
                            );
                        }
                        if focus == Some("tb-overflow") {
                            reached = true;
                            assert_eq!(
                                e2e::toolbar_label("tb-overflow"),
                                Some(kagi_ui_core::i18n::Msg::ToolbarOverflow.t())
                            );
                            break;
                        }
                    }
                    assert!(reached, "overflow must be a real toolbar Tab stop");
                    crate::keyboard_nav::keys(cx, win, "enter");
                    cx.run_until_parked();
                    cx.update_window(win, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    for (index, id) in IDS.iter().enumerate() {
                        let menu_item = e2e::control_bounds(
                            win.window_id(),
                            &format!("tb-overflow-item-{index}"),
                        );
                        assert_eq!(
                            menu_item.is_some(),
                            !visible[index],
                            "{language}/{display:?}/{id}: exact overflow membership"
                        );
                    }
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-shortcut-5").is_some(),
                        "Undo retains its actual shortcut"
                    );
                    let unavailable = e2e::control_bounds(win.window_id(), "tb-overflow-item-5")
                        .expect("Undo overflows before workspace modes");
                    cx.simulate_click(win, unavailable.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert!(cx.read(|cx| matches!(&app.read(cx).status_footer,
                        kagi::ui::FooterStatus::Idle(text) if text.as_ref() == kagi_ui_core::i18n::Msg::NothingToUndo.t()
                    )), "unavailable overflow action retains its localized reason");
                    assert!(!cx.read(|cx| e2e::active_modal_present(app.read(cx))));
                    cx.simulate_click(win, bounds.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    cx.update_window(win, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    if display == settings::ToolbarDisplay::IconAndText {
                        assert!(
                            !visible[2],
                            "Create Branch belongs in the constrained caption menu"
                        );
                        let item =
                            e2e::control_bounds(win.window_id(), "tb-overflow-item-2").unwrap();
                        cx.simulate_click(win, item.center(), gpui::Modifiers::none());
                        cx.run_until_parked();
                        assert!(
                            cx.read(|cx| e2e::active_modal_present(app.read(cx))),
                            "overflow Create Branch must open its real modal"
                        );
                    } else {
                        cx.simulate_keystrokes(win, "escape");
                        cx.run_until_parked();
                    }
                }
                unmount(cx, app, win);
            }
        }
    }
}
