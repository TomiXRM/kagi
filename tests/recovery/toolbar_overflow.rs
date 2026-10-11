//! #1210: narrow, zoomed toolbars expose clipped commands through overflow.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use gpui::{
    div, prelude::*, px, size, AnyWindowHandle, Context, Entity, VisualTestAppContext, Window,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(crate) struct ToolbarRestore {
    _presentation: crate::surface_scrollbars::PresentationRestore,
    saved: Option<crate::gui_isolation::SavedKeys>,
}

impl ToolbarRestore {
    pub(crate) fn capture(cx: &VisualTestAppContext) -> Self {
        Self {
            _presentation: crate::surface_scrollbars::PresentationRestore::capture(cx),
            saved: Some(crate::gui_isolation::SavedKeys::keep(&["toolbar_display"])),
        }
    }
}

impl Drop for ToolbarRestore {
    fn drop(&mut self) {
        drop(self.saved.take());
        settings::init_toolbar_display();
    }
}

/// #1179: document tabs do not occupy the native unified top frame.
pub fn scenario_unified_toolbar(cx: &mut VisualTestAppContext) {
    let _restore = ToolbarRestore::capture(cx);
    let first = build_fixture();
    let second = build_fixture();
    for slug in ["apple-light", "apple-dark", "catppuccin"] {
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for display in [
                    settings::ToolbarDisplay::IconOnly,
                    settings::ToolbarDisplay::IconAndText,
                ] {
                    settings::set_toolbar_display(display);
                    let state = e2e::app_state(first.path()).unwrap();
                    let captured = Rc::new(RefCell::new(None));
                    let output = captured.clone();
                    let win: AnyWindowHandle = crate::macos::open_offscreen(
                        cx,
                        size(px(1392.), px(883.)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    )
                    .into();
                    let app = captured.borrow().clone().unwrap();
                    app.update(cx, |app, cx| {
                        app.set_theme(slug, cx);
                        app.view_mut().status_summary.repo_name = "長い名前-repository".repeat(8);
                        app.view_mut().status_summary.branch = "feature/長いブランチ".repeat(8);
                        cx.notify();
                    });
                    kagi::ui::tab_panel_a11y::clear_recorded_tab_panels();
                    for _ in 0..5 {
                        frame(cx, win);
                    }
                    let bounds = |id| e2e::control_bounds(win.window_id(), id).unwrap();
                    let row = bounds("toolbar-row");
                    assert_eq!(
                        row.top(),
                        px(0.),
                        "unified frame must be first, not below tabs"
                    );
                    if zoom == 1. {
                        assert!(
                            row.size.height <= px(54.),
                            "100% unified frame ≤54 logical px"
                        );
                    }
                    assert!(
                        e2e::control_bounds(win.window_id(), "repo-tab-0").is_none(),
                        "single repository strip must be hidden"
                    );
                    assert!(
                        kagi::ui::tab_panel_a11y::recorded_tab_panel("repo-tab-panel").is_none(),
                        "hidden TabList must not leave a repository TabPanel"
                    );
                    for id in ["toolbar-identity", "tb-refresh", "tab-add"] {
                        let control = bounds(id);
                        contained(row, control, id);
                        assert!(
                            control.left() >= px(80.),
                            "native traffic-light exclusion: {id}"
                        );
                    }
                    let identity = bounds("toolbar-identity");
                    assert!(identity.size.width > px(30.), "identity remains readable");
                    let drag = bounds("toolbar-drag-region");
                    contained(row, drag, "blank draggable chrome");
                    assert!(drag.size.width > px(0.));
                    assert!(
                        drag.left() >= identity.right(),
                        "blank drag region follows identity"
                    );
                    for id in [
                        "tb-refresh",
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
                        "tb-overflow",
                        "tab-add",
                    ] {
                        if let Some(control) = e2e::control_bounds(win.window_id(), id) {
                            if control.size.width > px(0.) {
                                assert!(
                                    control.right() <= drag.left()
                                        || control.left() >= drag.right(),
                                    "{id} must not intersect the real blank drag target"
                                );
                            }
                        }
                    }
                    for id in ["tb-pull", "tb-push", "tb-graph-mode", "tb-overflow"] {
                        if let Some(control) = e2e::control_bounds(win.window_id(), id) {
                            if control.size.width > px(0.) {
                                contained(row, control, id);
                                assert!(
                                    control.left() >= identity.right(),
                                    "{id} overlaps identity"
                                );
                            }
                        }
                    }
                    cx.update_window(win, |_, window, cx| {
                        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
                    })
                    .unwrap();
                    cx.dispatch_action(win, kagi::ui::commands::NewTab);
                    cx.run_until_parked();
                    assert!(
                        app.read_with(cx, |app, _| app.home_in_front()),
                        "Cmd+T's command opens Home while the strip is hidden"
                    );
                    for _ in 0..5 {
                        frame(cx, win);
                    }
                    assert_eq!(
                        bounds("home-frame").size.height,
                        row.size.height,
                        "Home and workspace share presentation-mode frame height"
                    );
                    assert!(bounds("home-drag-region").size.width > px(0.));
                    cx.update_window(win, |_, window, cx| {
                        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
                    })
                    .unwrap();
                    cx.dispatch_action(win, kagi::ui::commands::CloseTab);
                    cx.run_until_parked();
                    assert!(!app.read_with(cx, |app, _| app.home_in_front()));
                    app.update(cx, |app, cx| {
                        assert!(app.open_repository(second.path().to_path_buf(), cx));
                    });
                    for _ in 0..5 {
                        frame(cx, win);
                    }
                    let tab = bounds("repo-tab-0");
                    assert!(tab.top() >= row.bottom(), "tabs below unified frame");
                    assert!(
                        tab.left() < px(80.),
                        "tabs no longer reserve native titlebar inset"
                    );
                    assert!(
                        kagi::ui::tab_panel_a11y::recorded_tab_panel("repo-tab-panel").is_some(),
                        "visible TabList retains repository TabPanel"
                    );
                    // The frame's + and command paths keep Home and close reachable.
                    let add = bounds("tab-add");
                    cx.simulate_click(win, add.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert!(app.read_with(cx, |app, _| app.home_in_front()));
                    cx.update_window(win, |_, window, cx| {
                        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
                    })
                    .unwrap();
                    cx.dispatch_action(win, kagi::ui::commands::CloseTab);
                    cx.run_until_parked();
                    assert!(!app.read_with(cx, |app, _| app.home_in_front()));
                    cx.update_window(win, |_, window, cx| {
                        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(1, window, cx));
                    })
                    .unwrap();
                    cx.dispatch_action(win, kagi::ui::commands::CloseTab);
                    for _ in 0..5 {
                        frame(cx, win);
                    }
                    assert_eq!(app.read_with(cx, |app, _| app.tabs.len()), 1);
                    // Recorder bounds must be cleared before checking disappearance.
                    e2e::clear_control_bounds(win.window_id(), "repo-tab-0");
                    frame(cx, win);
                    assert!(e2e::control_bounds(win.window_id(), "repo-tab-0").is_none());
                    assert!(
                        cx.update_window(win, |_, window, cx| {
                            app.read(cx).root_focus.as_ref().unwrap().is_focused(window)
                        })
                        .unwrap(),
                        "hiding focused strip releases focus to root"
                    );
                    cx.dispatch_action(win, kagi::ui::commands::CloseTab);
                    cx.run_until_parked();
                    e2e::clear_control_bounds(win.window_id(), "tab-add");
                    kagi::ui::tab_panel_a11y::clear_recorded_tab_panels();
                    for _ in 0..5 {
                        frame(cx, win);
                    }
                    assert!(
                        e2e::control_bounds(win.window_id(), "tab-add").is_none(),
                        "empty Home must not render inert +"
                    );
                    assert!(
                        kagi::ui::tab_panel_a11y::recorded_tab_panel("home-tab-panel").is_none(),
                        "hidden TabList must not leave a Home TabPanel"
                    );
                    unmount(cx, app, win);
                }
            }
        }
    }
}

pub fn scenario_toolbar_overflow(cx: &mut VisualTestAppContext) {
    let _restore = ToolbarRestore::capture(cx);
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
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-tooltip");
                    hover_overflow(cx, win, bounds.center());
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-tooltip").is_some(),
                        "closed overflow tooltip positive control"
                    );
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
                                for fragment in [
                                    "nothing to",
                                    "is clean",
                                    "configured",
                                    "is empty",
                                    "in progress",
                                ] {
                                    assert!(!reason.contains(fragment), "{id}: {reason}");
                                }
                            }
                        }
                    }
                    e2e::clear_control_bounds(win.window_id(), "tb-overflow-tooltip");
                    hover_overflow(cx, win, bounds.center());
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-tooltip").is_none(),
                        "open overflow must not render its trigger tooltip"
                    );
                    resize_overflow_window(cx, win, 480.);
                    for _ in 0..4 {
                        frame(cx, win);
                    }
                    assert!(
                        e2e::control_bounds(win.window_id(), "tb-overflow-menu")
                            .unwrap()
                            .size
                            .width
                            <= px(448.),
                        "open menu must refresh its cap after shrinking to 480px"
                    );
                    resize_overflow_window(cx, win, width);
                    for _ in 0..4 {
                        frame(cx, win);
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

fn hover_overflow(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.update_window(win, |_, window, cx| {
        window.dispatch_event(
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
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
}

fn resize_overflow_window(cx: &mut VisualTestAppContext, win: AnyWindowHandle, width: f32) {
    let wanted = size(px(width), px(660.));
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
}
