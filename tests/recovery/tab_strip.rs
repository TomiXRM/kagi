//! Repository strip overflow and close-action accessibility regressions.
use crate::macos::unmount;
use gpui::{AnyWindowHandle, MouseButton, Role, VisualTestAppContext, px, size};
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
    two_natural_tabs(cx, window, &app);
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
        // The native-light exclusion pushes twelve minimum-width tabs beyond
        // one viewport. Offscreen recorded cells are clipped by the real
        // horizontal scroll owner; selected/focused cells must be revealed.
        assert!(bounds.size.width > px(0.) && bounds.size.width <= theme::scaled_px(200.));
        assert!(
            e2e::control_paint(window.window_id(), &format!("repo-tab-label-{index}"))
                .unwrap()
                .middle_ellipsis
        );
    }
    let first = e2e::control_bounds(window.window_id(), "repo-tab-0").unwrap();
    crate::recovery_layout::contained(
        e2e::tab_strip::scroll().unwrap().bounds(),
        first,
        "active first tab revealed",
    );
    app.update(cx, |app, cx| app.switch_repo(11, cx));
    cx.run_until_parked();
    draw(cx, window);
    let last = e2e::control_bounds(window.window_id(), "repo-tab-11").unwrap();
    crate::recovery_layout::contained(
        e2e::tab_strip::scroll().unwrap().bounds(),
        last,
        "active last tab revealed",
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
    keyboard_tail(cx, window, &app);
    selection_reveal(cx, window, &app);
    wheel_axes(cx, window);
    retire_menu(cx, window, &app);
    unmount(cx, app, window);
}

fn retire_menu(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    click(cx, window, "tab-overflow");
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        while app.tabs.len() > 2 {
            app.close_tab(app.tabs.len() - 1, cx);
        }
    });
    e2e::clear_control_bounds(window.window_id(), "tab-overflow");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "tab-overflow").is_none(),
        "» disappears again when the remaining natural-width tabs fit"
    );
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window)
        })
        .unwrap(),
        "retired overflow returns focus to the root"
    );
    app.update(cx, |app, cx| app.close_tab(1, cx));
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "tab-overflow").is_none());
}

fn keyboard_tail(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    // End reveals the focused cell before manual activation above the floor.
    theme::set_zoom(1.67);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    draw(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "end");
    draw(cx, window);
    let last = e2e::control_bounds(window.window_id(), "repo-tab-11").unwrap();
    let viewport = e2e::tab_strip::scroll().unwrap().bounds();
    assert!(
        last.left() >= viewport.left() && last.right() <= viewport.right(),
        "keyboard tail not revealed: {last:?} viewport {viewport:?}"
    );
    let close = e2e::control_bounds(window.window_id(), "tab-close-11").unwrap();
    assert!(close.size.width >= px(20.) && close.size.height >= px(20.));
    crate::keyboard_nav::keys(cx, window, "enter");
    assert_eq!(app.read_with(cx, |app, _| app.active_tab), 11);
}

fn selection_reveal(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    draw(cx, window);
    app.update(cx, |app, cx| app.switch_repo(11, cx));
    cx.run_until_parked();
    draw(cx, window);
    let last = e2e::control_bounds(window.window_id(), "repo-tab-11").unwrap();
    let viewport = e2e::tab_strip::scroll().unwrap().bounds();
    assert!(
        last.left() >= viewport.left() && last.right() <= viewport.right(),
        "a newly active tab outranks old keyboard focus and stays revealed"
    );
}

fn wheel_axes(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let scroll = e2e::tab_strip::scroll().unwrap();
    let before = scroll.offset();
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(200.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        scroll.offset(),
        before,
        "vertical wheel must not scroll tabs"
    );
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(200.), px(0.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    draw(cx, window);
    assert_ne!(
        scroll.offset(),
        before,
        "horizontal wheel still scrolls tabs"
    );
}

pub fn scenario_tab_close_names(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixtures: Vec<_> = (0..3).map(|_| crate::macos::build_fixture()).collect();
    let paths: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            crate::macos::git(
                fixture.path(),
                &["clone", "-q", ".", "same-repository-with-a-long-name"],
            );
            fixture.path().join("same-repository-with-a-long-name")
        })
        .collect();
    let (app, window) = mount(cx, &paths[0]);
    for path in paths.iter().skip(1) {
        app.update(cx, |app, cx| assert!(app.open_repository(path.clone(), cx)));
        cx.run_until_parked();
    }
    app.update(cx, |app, _| {
        app.tabs[0].remote = Some(kagi::ui::RemoteRepoView {
            host: kagi_domain::remote::RemoteHost::parse("example.test").unwrap(),
            root: "/srv/same-repository".into(),
        });
        app.tabs[1].is_worktree = true;
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    theme::set_zoom(1.67);
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        draw(cx, window);
        for index in 0..3 {
            assert_eq!(
                e2e::tab_strip::control(&format!("tab-close-{index}")),
                Some((
                    Role::Button,
                    if lang == i18n::Lang::En {
                        "Close same-repository-with-a-long-name"
                    } else {
                        "same-repository-with-a-long-name を閉じる"
                    }
                    .to_owned()
                ))
            );
            let description = e2e::tab_strip::description(&format!("tab-close-{index}"))
                .expect("close describes its own repository path");
            assert_eq!(
                std::path::Path::new(&description).canonicalize().unwrap(),
                paths[index].canonicalize().unwrap()
            );
        }
        assert_eq!(
            e2e::tab_strip::control("home-tab-close"),
            Some((
                Role::Button,
                if lang == i18n::Lang::En {
                    "Close Home"
                } else {
                    "ホーム を閉じる"
                }
                .to_owned()
            ))
        );
        duplicate_menu(cx, window, &app);
    }
    unmount(cx, app, window);
}

fn duplicate_menu(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    click(cx, window, "tab-overflow");
    cx.run_until_parked();
    draw(cx, window);
    let paths = app.read_with(cx, |app, _| {
        app.tabs
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>()
    });
    for (index, path) in paths.iter().enumerate() {
        let id = format!("tab-overflow-item-{index}");
        let marker = match index {
            0 => "☁ ",
            1 => "🌲 ",
            _ => "",
        };
        assert_eq!(
            e2e::tab_strip::control(&id),
            Some((
                Role::MenuItem,
                format!(
                    "{marker}same-repository-with-a-long-name — {}",
                    path.parent().unwrap().display()
                )
            ))
        );
        assert_eq!(
            e2e::tab_strip::description(&id),
            Some(path.display().to_string())
        );
    }
    crate::keyboard_nav::keys(cx, window, "escape");
    draw(cx, window);
}

fn two_natural_tabs(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    let old_theme = theme::theme().slug.clone();
    app.update(cx, |app, cx| {
        app.tabs[0].name = "stress-pr-repo".into();
        app.set_theme("apple-light", cx);
    });
    theme::set_zoom(1.67);
    i18n::set_lang(i18n::Lang::Ja);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "tab-overflow");
    let mut natural_widths = None;
    let mut blank_width = None;
    for width in [940., 1200.] {
        for _ in 0..4 {
            crate::recovery_layout::draw(cx, window, (width, 660.));
        }
        let repo = e2e::control_bounds(window.window_id(), "repo-tab-0").unwrap();
        let home = e2e::control_bounds(window.window_id(), "home-tab").unwrap();
        let scroll = e2e::tab_strip::scroll().unwrap();
        // The tab scroll viewport intentionally hugs its natural-width items.
        // Unused chrome belongs to the separate measured drag spacer.
        let blank = e2e::control_bounds(window.window_id(), "titlebar-drag-region").unwrap();
        assert!(
            blank.left() >= scroll.bounds().right() && blank.size.width > px(0.),
            "unused chrome {blank:?} follows the natural tab viewport {:?}",
            scroll.bounds()
        );
        if let Some(previous) = blank_width {
            assert!(
                blank.size.width > previous,
                "the wider parent gives unused space to chrome, not stretched tabs"
            );
        }
        blank_width = Some(blank.size.width);
        let widths = (repo.size.width, home.size.width);
        if let Some(previous) = natural_widths {
            assert_eq!(
                widths, previous,
                "the same two tabs must not stretch when an ample viewport grows"
            );
        }
        natural_widths = Some(widths);
        assert_eq!(
            scroll.bounds_for_item(1).unwrap().left(),
            scroll.bounds_for_item(0).unwrap().right() + px(1.)
        );
        assert!(
            e2e::control_bounds(window.window_id(), "tab-overflow").is_none(),
            "no » when both natural-width tabs fit"
        );
        // Sample the actual tab interiors and unused chrome, rather than pin
        // incidental label widths that change with the fixed-chrome type ramp.
        cx.update_window(window, |_, window, _| {
            let image = window.render_to_image().unwrap();
            let scale = window.scale_factor();
            for (x, color, label) in [
                (
                    repo.left() + px(6.),
                    theme::theme().surface,
                    "inactive repository",
                ),
                (home.center().x, theme::theme().bg_base, "active Home"),
                (
                    blank.center().x,
                    theme::theme().chrome_surface(),
                    "natural-width trailing chrome",
                ),
            ] {
                let pixel = image
                    .get_pixel(
                        (f32::from(x) * scale) as u32,
                        ((f32::from(repo.top()) + 5.) * scale) as u32,
                    )
                    .0;
                assert_eq!(
                    &pixel[..3],
                    &[(color >> 16) as u8, (color >> 8) as u8, color as u8],
                    "{label}"
                );
            }
        })
        .unwrap();
    }
    app.update(cx, |app, cx| {
        app.close_home_tab(cx);
        app.set_theme(&old_theme, cx);
    });
    i18n::set_lang(i18n::Lang::En);
    theme::set_zoom(1.);
    cx.run_until_parked();
    draw(cx, window);
}
