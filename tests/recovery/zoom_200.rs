//! #1553: the existing global zoom reaches 200% without losing essential UI.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Focusable, Pixels, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    for _ in 0..5 {
        e2e::clear_control_bounds(win.window_id(), "tb-overflow");
        for index in 0..13 {
            e2e::clear_control_bounds(win.window_id(), &format!("tb-overflow-item-{index}"));
        }
        crate::macos::drain_native_events();
        cx.run_until_parked();
        cx.update_window(win, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

fn bounds(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    crate::settings_controls::bounds(cx, win, id).unwrap_or_else(|| panic!("missing {id}"))
}

fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let at = bounds(cx, win, id).center();
    cx.simulate_mouse_move(win, at, None, gpui::Modifiers::none());
    draw(cx, win);
    cx.simulate_click(win, at, gpui::Modifiers::none());
    draw(cx, win);
}

fn root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
}

fn usable(cx: &mut VisualTestAppContext, viewport: Bounds<Pixels>, win: AnyWindowHandle, id: &str) {
    let control = bounds(cx, win, id);
    assert!(
        control.size.width > px(0.) && control.size.height > px(0.),
        "{id}: {control:?}"
    );
    contained(viewport, control, id);
}

fn reveal_zoom(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    // Reveal the actual shared stepper inside the detail-only scroll owner.
    let control = bounds(cx, win, "zoom-dec");
    let page = bounds(cx, win, "settings-detail-scroll");
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: page.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), page.top() + px(20.) - control.top())),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, win);
    usable(cx, page, win, "zoom-dec");
    usable(cx, page, win, "zoom-inc");
}

fn settings_window(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: AnyWindowHandle) {
    root(cx, app, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    draw(cx, repo);
    let win = cx
        .read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
        })
        .expect("production Settings opens separately");
    draw(cx, win);
    let viewport = cx
        .update_window(win, |_, window, _| {
            Bounds::new(point(px(0.), px(0.)), window.viewport_size())
        })
        .unwrap();
    for pane in kagi_ui_settings::Pane::ALL {
        let id = format!("settings-pane-{}", pane.id());
        usable(cx, viewport, win, &id);
        click(cx, win, &id);
        assert_eq!(
            settings::read_setting("settings_pane").as_deref(),
            Some(pane.id())
        );
    }
    click(cx, win, "settings-pane-appearance");
    assert_eq!(
        crate::settings_controls::probe(cx, win, "zoom-value")
            .unwrap()
            .value,
        "200%"
    );
    reveal_zoom(cx, win);
    assert!(
        crate::settings_controls::probe(cx, win, "zoom-inc")
            .unwrap()
            .disabled
    );
    crate::native_ax::activate(cx);
    draw(cx, win);
    let decrement = crate::native_ax::node(cx, win, "zoom-dec".into());
    assert_eq!(decrement["aria"]["role"], "Button");
    assert_eq!(
        decrement["aria"]["label"],
        i18n::settings_zoom_action(false, "200%")
    );
    click(cx, win, "zoom-dec");
    assert_eq!(theme::zoom(), 1.9);
    reveal_zoom(cx, win);
    click(cx, win, "zoom-inc");
    assert_eq!(theme::zoom(), 2.);
    assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some("2000"));
    cx.update_window(win, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    theme::set_zoom(1.);
    settings::write_setting("ui_zoom", Some("2000"));
    theme::init_zoom();
    assert_eq!(theme::zoom(), 2., "saved 200% survives startup loading");
    root(cx, app, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    draw(cx, repo);
    let reopened = cx
        .read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
        })
        .unwrap();
    draw(cx, reopened);
    assert_eq!(
        crate::settings_controls::probe(cx, reopened, "zoom-value")
            .unwrap()
            .value,
        "200%"
    );
    cx.update_window(reopened, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
}

pub fn scenario_zoom_200_operable(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "settings_pane",
        "settings_window_frame",
        "ui_scale_base",
        "commit_template_enabled",
    ]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("zoom-wip.txt"), "200% WIP\n").unwrap();
    std::fs::write(
        fixture.path().join("template"),
        "# Zoom regression template\n",
    )
    .unwrap();
    crate::macos::git(fixture.path(), &["config", "commit.template", "template"]);
    assert_eq!(
        theme::set_zoom(2.),
        2.,
        "the published UI zoom must reach 200%"
    );
    settings::set_toolbar_display(settings::ToolbarDisplay::IconAndText);
    for (width, height) in [(1000., 720.), (1400., 900.), (940., 720.)] {
        for locale in [i18n::Lang::En, i18n::Lang::Ja] {
            for preset in ["apple-light", "apple-dark"] {
                i18n::set_lang(locale);
                theme::set_active(preset);
                theme::set_zoom(2.);
                let state = e2e::app_state(fixture.path()).unwrap();
                theme::set_zoom(2.);
                cx.update(theme::sync_gpui_component_theme);
                let captured = Rc::new(RefCell::new(None));
                let output = captured.clone();
                let win: AnyWindowHandle = crate::macos::open_offscreen(
                    cx,
                    size(px(width), px(height)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                )
                .into();
                let app = captured.borrow_mut().take().unwrap();
                draw(cx, win);
                let viewport = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
                assert_eq!(theme::rem_size_px(), theme::BASE_REM_PX * 2.);
                for id in ["toolbar-row", "tb-graph-mode"] {
                    usable(cx, viewport, win, id);
                }
                if e2e::control_bounds(win.window_id(), "tb-overflow").is_some() {
                    usable(cx, viewport, win, "tb-overflow");
                    crate::toolbar_overflow::open_keyboard_overflow(cx, &app, win);
                    draw(cx, win);
                    usable(cx, viewport, win, "tb-overflow-item-11");
                    click(cx, win, "tb-overflow-item-11");
                    assert!(
                        cx.read(|cx| app.read(cx).ui().ecosystem.is_some()),
                        "overflow activates the actual Analyze workspace"
                    );
                    assert_eq!(
                        cx.read(|cx| app.read(cx).workspace_mode()),
                        kagi::ui::workspace_mode::WorkspaceMode::Takeover
                    );
                    app.update(cx, |app, cx| app.show_graph_mode(cx));
                    draw(cx, win);
                    assert!(
                        (0..13).all(|index| e2e::control_bounds(
                            win.window_id(),
                            &format!("tb-overflow-item-{index}")
                        )
                        .is_none()),
                        "choosing an overflow action dismisses the menu"
                    );
                }
                if kagi_domain::compact::is_compact(
                    width,
                    theme::rem_size_px(),
                    theme::compact_threshold_rem(),
                ) {
                    usable(cx, viewport, win, "compact-rail");
                    for id in [
                        "compact-rail-local",
                        "compact-rail-remote",
                        "compact-rail-worktrees",
                        "compact-rail-tags",
                        "compact-rail-stashes",
                        "compact-rail-prs",
                        "compact-rail-issues",
                    ] {
                        usable(cx, viewport, win, id);
                    }
                }
                root(cx, &app, win);
                cx.simulate_keystrokes(win, "cmd-f");
                draw(cx, win);
                usable(cx, viewport, win, "sidebar-filter");
                let filter = cx.read(|cx| app.read(cx).sidebar.filter.clone()).unwrap();
                assert!(cx
                    .update_window(win, |_, window, cx| filter
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window))
                    .unwrap());
                cx.simulate_input(win, "zoom-search");
                draw(cx, win);
                assert_eq!(
                    cx.read(|cx| filter.read(cx).value().to_string()),
                    "zoom-search"
                );
                cx.simulate_keystrokes(win, "cmd-a backspace escape");
                drop(filter);
                root(cx, &app, win);
                cx.update_window(win, |_, window, cx| {
                    app.update(cx, |app, cx| app.open_commit_panel(window, cx))
                })
                .unwrap();
                draw(cx, win);
                for id in [
                    "cp-body",
                    "cp-template-toggle-hit",
                    "cp-smart-suggest-hit",
                    "cp-coauthor-hit",
                    "cp-amend-btn-hit",
                    "cp-commit-action",
                ] {
                    usable(cx, viewport, win, id);
                }
                let panel = cx
                    .read(|cx| app.read(cx).ui().commit_panel.clone())
                    .unwrap();
                let body = cx.read(|cx| panel.read(cx).body_input.clone()).unwrap();
                cx.update_window(win, |_, window, cx| {
                    body.update(cx, |input, cx| input.focus(window, cx))
                })
                .unwrap();
                cx.simulate_input(win, "editable at 200%");
                draw(cx, win);
                assert!(cx.read(|cx| body.read(cx).value().contains("editable at 200%")));
                for id in [
                    "cp-template-toggle-hit",
                    "cp-smart-suggest-hit",
                    "cp-coauthor-hit",
                    "cp-amend-btn-hit",
                ] {
                    contained(bounds(cx, win, "cp-body"), bounds(cx, win, id), id);
                }
                assert!(bounds(win, "cp-body").bottom() <= bounds(win, "cp-commit-action").top());
                let template_on =
                    e2e::commit_panel_labels::accessible_pressed("cp-template-toggle");
                click(cx, win, "cp-template-toggle-hit");
                assert_ne!(
                    e2e::commit_panel_labels::accessible_pressed("cp-template-toggle"),
                    template_on,
                    "Template remains operable"
                );
                click(cx, win, "cp-coauthor-hit");
                assert!(
                    cx.read(|cx| panel.read(cx).coauthor_menu.is_some()),
                    "co-author remains operable"
                );
                cx.simulate_keystrokes(win, "escape");
                draw(cx, win);
                drop(body);
                drop(panel);
                root(cx, &app, win);
                cx.simulate_keystrokes(win, "escape");
                draw(cx, win);
                settings_window(cx, &app, win);
                root(cx, &app, win);
                cx.simulate_keystrokes(win, "cmd--");
                draw(cx, win);
                assert_eq!(theme::zoom(), 1.9, "Zoom Out shortcut");
                cx.simulate_keystrokes(win, "cmd-=");
                draw(cx, win);
                assert_eq!(theme::zoom(), 2., "Zoom In shortcut reaches 200%");
                cx.simulate_keystrokes(win, "cmd-0");
                draw(cx, win);
                assert_eq!(theme::zoom(), 1., "Zoom Reset preserves 100%");
                eprintln!("[gui-e2e] zoom_200_operable {locale:?}/{preset}/{width}x{height}");
                unmount(cx, app, win);
            }
        }
    }
    eprintln!("[gui-e2e] PASS zoom_200_operable");
}
