//! #1210: narrow, zoomed toolbars expose clipped commands through overflow.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::{contained, GlobalSettings};
use gpui::{
    div, prelude::*, px, size, AnyWindowHandle, Context, Entity, VisualTestAppContext, Window,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub fn scenario_toolbar_overflow(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _settings = crate::gui_isolation::SavedKeys::keep(&["toolbar_display", "theme"]);
    let original_theme = theme::theme();
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
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
                    for index in 0..IDS.len() {
                        if let Some(shortcut) = e2e::control_bounds(
                            win.window_id(),
                            &format!("tb-overflow-shortcut-{index}"),
                        ) {
                            let row = e2e::control_bounds(
                                win.window_id(),
                                &format!("tb-overflow-item-{index}"),
                            )
                            .unwrap();
                            contained(viewport, shortcut, "overflow shortcut inside window");
                            contained(
                                e2e::control_bounds(win.window_id(), "tb-overflow-menu").unwrap(),
                                shortcut,
                                "shortcut inside actual popup menu",
                            );
                            contained(row, shortcut, "overflow shortcut inside its row");
                            assert!(
                                shortcut.top() <= row.top() + px(1.),
                                "shortcut must occupy the label's first line: {row:?} / {shortcut:?}"
                            );
                        }
                    }
                    let menu_bounds =
                        e2e::control_bounds(win.window_id(), "tb-overflow-menu").unwrap();
                    assert!(menu_bounds.size.width <= px(width - 32.));
                    if language == "ja" {
                        for id in [
                            "tb-pull", "tb-push", "tb-stash", "tb-pop", "tb-undo", "tb-redo",
                        ] {
                            if let Some(reason) = e2e::toolbar_description(id) {
                                assert!(
                                    !reason.chars().any(|ch| ch.is_ascii_alphabetic()),
                                    "{id} reason must be Japanese: {reason}"
                                );
                            }
                        }
                    }
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-tooltip");
                    cx.update_window(win, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: bounds.center(),
                                pressed_button: None,
                                modifiers: gpui::Modifiers::none(),
                            }),
                            cx,
                        );
                    })
                    .unwrap();
                    cx.advance_clock(std::time::Duration::from_secs(1));
                    cx.run_until_parked();
                    frame(cx, win);
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-tooltip").is_none(),
                        "open overflow must not render its trigger tooltip"
                    );
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
                    // A language/zoom refresh must neither temporarily dismiss
                    // the open menu nor retain its old unavailable-reason snapshot.
                    open_keyboard_overflow(cx, &app, win);
                    std::env::set_var("KAGI_LANG", if language == "en" { "ja" } else { "en" });
                    i18n::init_lang();
                    theme::set_zoom(1.66);
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-item-5");
                    frame(cx, win);
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-item-5").is_some(),
                        "pending style measurement must preserve the open overflow menu"
                    );
                    for _ in 0..4 {
                        frame(cx, win);
                    }
                    let undo = e2e::control_bounds(win.window_id(), "tb-overflow-item-5").unwrap();
                    cx.simulate_click(win, undo.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert!(cx.read(|cx| matches!(&app.read(cx).status_footer,
                        kagi::ui::FooterStatus::Idle(text) if text.as_ref() == kagi_ui_core::i18n::Msg::NothingToUndo.t()
                    )), "an open overflow menu must use refreshed unavailable reasons");
                    std::env::set_var("KAGI_LANG", language);
                    i18n::init_lang();
                    theme::set_zoom(1.67);
                    for _ in 0..4 {
                        frame(cx, win);
                    }
                    // Closing an overflow item must not undo focus explicitly
                    // assigned by the action (in particular the real PTY view).
                    // Kagi mounts with the Terminal panel visible by default;
                    // start closed so this activation opens rather than toggles it off.
                    app.update(cx, |app, cx| {
                        app.bottom_panel_open = false;
                        cx.notify();
                    });
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-item-7");
                    open_keyboard_overflow(cx, &app, win);
                    let terminal = e2e::control_bounds(win.window_id(), "tb-overflow-item-7")
                        .expect("Terminal belongs in constrained overflow");
                    cx.simulate_click(win, terminal.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    frame(cx, win);
                    assert!(
                        cx.read(|cx| app.read(cx).bottom_panel_open),
                        "overflow Terminal click at {terminal:?} must open the real panel"
                    );
                    assert!(
                        cx.update_window(win, |_, window, cx| {
                            app.read(cx).terminal_focused_for_e2e(window, cx)
                        })
                        .unwrap(),
                        "overflow Terminal must retain PTY focus"
                    );
                    cx.update_window(win, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.bottom_panel_open = false;
                            app.root_focus.clone().unwrap().focus(window, cx);
                            cx.notify();
                        });
                    })
                    .unwrap();
                    frame(cx, win);
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-item-2");
                    let trigger = e2e::control_bounds(win.window_id(), "tb-overflow").unwrap();
                    cx.simulate_click(win, trigger.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    frame(cx, win);
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
                    // Force-open uses gpui-kit's deferred registration too.
                    // Removing the trigger while this menu is open must release
                    // that registration before the element state is discarded.
                    cx.simulate_keystrokes(win, "escape");
                    cx.run_until_parked();
                    open_keyboard_overflow(cx, &app, win);
                    assert!(e2e::control_bounds(win.window_id(), "tb-overflow-item-5").is_some());
                    theme::set_zoom(1.);
                    let wanted = size(px(1392.), px(660.));
                    cx.update_window(win, |_, window, _| window.resize(wanted))
                        .unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                    loop {
                        crate::macos::drain_native_events();
                        cx.run_until_parked();
                        if cx
                            .update_window(win, |_, window, _| window.viewport_size())
                            .unwrap()
                            == wanted
                        {
                            break;
                        }
                        assert!(
                            std::time::Instant::now() < deadline,
                            "native resize timed out"
                        );
                    }
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow");
                    for _ in 0..4 {
                        frame(cx, win);
                    }
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow").is_none(),
                        "wide toolbar removes overflow"
                    );
                    assert_text_context_available(cx);
                }
                unmount(cx, app, win);
            }
        }
    }
    assert!(theme::set_active(&original_theme.slug));
    drop(_settings);
    settings::init_toolbar_display();
}

fn frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn open_keyboard_overflow(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) {
    cx.update_window(win, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
    for _ in 0..120 {
        cx.update_window(win, |_, window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear();
        })
        .unwrap();
        let _ = e2e::take_focus_probe();
        cx.simulate_keystrokes(win, "f19");
        if e2e::take_focus_probe() == Some("tb-overflow") {
            crate::keyboard_nav::keys(cx, win, "enter");
            cx.run_until_parked();
            frame(cx, win);
            return;
        }
    }
    panic!("overflow keyboard trigger was not reachable");
}

struct ContextProbe {
    input: Entity<gpui_component::input::InputState>,
    opened: Rc<Cell<bool>>,
}

impl Render for ContextProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let opened = self.opened.clone();
        div()
            .size_full()
            .child(gpui_component::input::Input::new(&self.input).context_menu(
                move |menu, _, _| {
                    // Input's real right-click path checks deferred context before
                    // invoking this builder. This observes that guard without
                    // exposing gpui-kit's private global registry to the test.
                    opened.set(true);
                    menu
                },
            ))
    }
}

fn assert_text_context_available(cx: &mut VisualTestAppContext) {
    let opened = Rc::new(Cell::new(false));
    let output = opened.clone();
    let win: AnyWindowHandle =
        crate::macos::open_offscreen(cx, size(px(300.), px(100.)), move |window, cx| {
            let input = cx.new(|cx| gpui_component::input::InputState::new(window, cx));
            let probe = cx.new(|_| ContextProbe {
                input,
                opened: output,
            });
            cx.new(|cx| gpui_component::Root::new(probe, window, cx))
        })
        .into();
    frame(cx, win);
    let point = gpui::point(px(40.), px(12.));
    cx.simulate_mouse_down(
        win,
        point,
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        win,
        point,
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(opened.get(), "widening an open toolbar must not strand deferred context and block text right-click menus");
    cx.update_window(win, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
}
