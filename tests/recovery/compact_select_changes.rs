//! #1528: user graph selection activates Changes only in compact windows.
use crate::macos::{build_fixture, git, mount_state, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab, KagiApp};
use kagi_ui_core::{i18n, theme};

pub fn scenario_compact_select_changes(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("selection.txt"),
        "selected commit file\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "."]);
    git(
        fixture.path(),
        &["commit", "-qm", "Selection detail 日本語"],
    );
    std::fs::write(fixture.path().join("wip.txt"), "working tree\n").unwrap();
    let before = crate::macos::repo_fingerprint(fixture.path());
    for locale in ["en", "ja"] {
        for zoom in [1., 1.67] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            let state = e2e::app_state(fixture.path()).unwrap();
            theme::set_zoom(zoom);
            theme::set_active("apple-light");
            cx.update(theme::sync_gpui_component_theme);
            let (app, win) = mount_state(cx, state);
            resize(cx, win, 900.);
            click(cx, win, "tab-terminal");
            click(cx, win, "graph-row-0");
            assert_changes(cx, &app, win, 0, locale, zoom);
            click(cx, win, "tab-terminal");
            click(cx, win, "graph-row-1");
            assert_changes(cx, &app, win, 1, locale, zoom);
            click(cx, win, "tab-terminal");
            click(cx, win, "graph-row-1");
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "same-row pointer reselect must not replace Terminal"
            );
            // The established pointer path toggles selection off on reselect.
            assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
            click(cx, win, "graph-row-0");
            assert_changes(cx, &app, win, 0, locale, zoom);
            click(cx, win, "tab-terminal");
            focus_graph(cx, &app, win);
            key(cx, win, "down");
            assert_changes(cx, &app, win, 1, locale, zoom);
            click(cx, win, "tab-terminal");
            focus_graph(cx, &app, win);
            key(cx, win, "up");
            assert_changes(cx, &app, win, 0, locale, zoom);
            click(cx, win, "tab-terminal");
            focus_graph(cx, &app, win);
            key(cx, win, "up");
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "clamped same-row keyboard selection keeps Terminal"
            );
            app.update(cx, |app, cx| app.reload(cx));
            settle(cx, win);
            assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "refresh retaining selection must not activate Changes"
            );
            click(cx, win, "compact-wip-row");
            assert_eq!(cx.read(|cx| app.read(cx).bottom_tab), BottomTab::Changes);
            assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
            for id in ["compact-changes-unstaged", "compact-changes-staged"] {
                assert!(
                    e2e::control_bounds(win.window_id(), id).is_some(),
                    "WIP renders {id}"
                );
            }
            // Close the pinned composer before testing the wide pointer path.
            focus_graph(cx, &app, win);
            key(cx, win, "escape");
            resize(cx, win, 1000.);
            click(cx, win, "tab-terminal");
            click(cx, win, "graph-row-0");
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "desktop exact-1000px pointer selection is unchanged"
            );
            focus_graph(cx, &app, win);
            key(cx, win, "down");
            assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "desktop keyboard selection is unchanged"
            );
            unmount(cx, app, win);
        }
    }
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
}

fn assert_changes(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    row: usize,
    locale: &str,
    zoom: f32,
) {
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(row));
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_tab),
        BottomTab::Changes,
        "{locale}/{zoom}: changed graph selection must activate Changes"
    );
    assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
    for id in [
        "compact-inspector-details",
        "compact-inspector-files",
        "inspector-file-0",
    ] {
        assert!(
            e2e::control_bounds(win.window_id(), id).is_some(),
            "selected commit renders {id}"
        );
    }
    let rendered = e2e::inspector_metadata::last().expect("selected commit metadata renderer");
    let (author, short) = cx.read(|cx| {
        let selected = &app.read(cx).view().rows[row];
        (selected.author.clone(), selected.short_id.clone())
    });
    assert_eq!(rendered.author, author);
    assert_eq!(
        rendered.hash_name.as_deref(),
        Some(i18n::inspector_copy_hash(&short).as_str())
    );
    assert_eq!(
        e2e::text_caps::message().source.as_ref().trim_end(),
        if row == 0 {
            "Selection detail 日本語"
        } else {
            "second commit"
        }
    );
    let files = kagi::ui::list_a11y::recorded_list("inspector-files-scroll").unwrap();
    let path = if row == 0 {
        "selection.txt"
    } else {
        "README.md"
    };
    assert!(
        files.rows.values().any(|(name, _)| name.ends_with(path)),
        "Changes lists the selected commit's real changed file {path}"
    );
}

fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    for id in [
        "compact-inspector-details",
        "compact-inspector-files",
        "inspector-file-0",
        "compact-changes-unstaged",
        "compact-changes-staged",
    ] {
        e2e::clear_control_bounds(win.window_id(), id);
    }
    e2e::inspector_metadata::clear();
    e2e::text_caps::clear();
    kagi::ui::list_a11y::clear_recorded_lists();
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(win, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}
fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, width: f32) {
    let wanted = size(px(width), px(720.));
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
            "native resize must reach {wanted:?}"
        );
    }
    settle(cx, win);
}
fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let bounds = e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("missing {id}"));
    cx.simulate_mouse_move(win, bounds.center(), None, Modifiers::none());
    cx.simulate_click(win, bounds.center(), Modifiers::none());
    settle(cx, win);
}
fn focus_graph(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
    cx.update_window(win, |_, window, cx| root.focus(window, cx))
        .unwrap();
    settle(cx, win);
}
fn key(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
    settle(cx, win);
}
