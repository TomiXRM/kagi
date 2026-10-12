//! Repository strip overflow and close-action accessibility regressions.
use crate::macos::unmount;
use gpui::{px, size, AnyWindowHandle, MouseButton, Role, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

fn mount(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (gpui::Entity<kagi::ui::KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(cx, size(px(940.), px(660.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    })
    .into();
    let app = captured.borrow_mut().take().unwrap();
    cx.run_until_parked();
    (app, window)
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..4 {
        crate::recovery_layout::draw(cx, window, (940., 660.));
    }
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let bounds = e2e::control_bounds(window.window_id(), id).expect("rendered named control");
    cx.simulate_mouse_down(
        window,
        bounds.center(),
        MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_up(
        window,
        bounds.center(),
        MouseButton::Left,
        Default::default(),
    );
}

pub fn scenario_tab_strip_overflow(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    theme::set_zoom(1.);
    let fixtures: Vec<_> = (0..12).map(|_| crate::macos::build_fixture()).collect();
    let (app, window) = mount(cx, fixtures[0].path());
    for fixture in fixtures.iter().skip(1) {
        app.update(cx, |app, cx| {
            assert!(app.open_repository(fixture.path().to_path_buf(), cx));
        });
        cx.run_until_parked();
    }
    app.update(cx, |app, cx| {
        for (index, tab) in app.tabs.iter_mut().enumerate() {
            tab.name = format!("repository-with-a-very-long-name-{index}");
        }
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    draw(cx, window);
    for index in 0..12 {
        let bounds = e2e::control_bounds(window.window_id(), &format!("repo-tab-{index}")).unwrap();
        assert!(
            bounds.left() >= px(0.) && bounds.right() <= px(940.),
            "tab {index} runs outside window: {bounds:?}"
        );
    }
    app.update(cx, |app, cx| app.switch_repo(11, cx));
    cx.run_until_parked();
    draw(cx, window);
    let last = e2e::control_bounds(window.window_id(), "repo-tab-11").unwrap();
    assert!(
        last.left() >= px(0.) && last.right() <= px(940.),
        "active last tab not revealed: {last:?}"
    );
    click(cx, window, "tab-overflow");
    cx.run_until_parked();
    draw(cx, window);
    for index in 0..12 {
        let (_, label) = e2e::tab_strip::control(&format!("tab-overflow-item-{index}"))
            .expect("all tabs are menu items");
        assert_eq!(label, format!("repository-with-a-very-long-name-{index}"));
    }
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    // End must reveal the focused cell before manual activation, even above
    // the strip's minimum-width threshold.
    theme::set_zoom(1.67);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    draw(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    cx.simulate_keystrokes(window, "end");
    draw(cx, window);
    let last = e2e::control_bounds(window.window_id(), "repo-tab-11").unwrap();
    let viewport = e2e::tab_strip::scroll().unwrap().bounds();
    assert!(
        last.left() >= viewport.left() && last.right() <= viewport.right(),
        "keyboard tail not revealed: {last:?} viewport {viewport:?}; focused={:?}",
        cx.update_window(window, |_, window, cx| app
            .read(cx)
            .tab_strip_focused_for_e2e(window))
            .unwrap()
    );
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert_eq!(app.read_with(cx, |app, _| app.active_tab), 11);
    unmount(cx, app, window);
}

pub fn scenario_tab_close_names(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixtures: Vec<_> = (0..3).map(|_| crate::macos::build_fixture()).collect();
    let (app, window) = mount(cx, fixtures[0].path());
    for fixture in fixtures.iter().skip(1) {
        app.update(cx, |app, cx| {
            assert!(app.open_repository(fixture.path().to_path_buf(), cx));
        });
        cx.run_until_parked();
    }
    app.update(cx, |app, _| {
        for tab in &mut app.tabs {
            tab.name = "same-repository".into();
        }
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        draw(cx, window);
        for index in 0..3 {
            assert_eq!(
                e2e::tab_strip::control(&format!("tab-close-{index}")),
                Some((
                    Role::Button,
                    if lang == i18n::Lang::En {
                        "Close same-repository"
                    } else {
                        "same-repository を閉じる"
                    }
                    .to_owned()
                ))
            );
        }
        assert_eq!(
            e2e::tab_strip::control("home-tab-close"),
            Some((
                Role::Button,
                if lang == i18n::Lang::En {
                    "Close Home"
                } else {
                    "Home を閉じる"
                }
                .to_owned()
            ))
        );
    }
    unmount(cx, app, window);
}
