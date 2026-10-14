//! #1567: removing a Recent item edits history, never its repository or tabs.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, MouseButton, MouseDownEvent, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, settings, tabs, KagiApp};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn key(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
    draw(cx, window);
}

fn focus_recent(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for _ in 0..40 {
        key(cx, window, "tab");
        let focused = cx
            .update_window(window, |_, w, cx| {
                app.read(cx)
                    .list_row_focused_for_e2e("home-recent-list", w)
                    .is_some()
            })
            .unwrap();
        if focused {
            return;
        }
    }
    panic!("Recent list must be reachable by Tab");
}

fn middle_removal(cx: &mut VisualTestAppContext, lang: Lang) {
    let repos = (0..12).map(|_| build_fixture()).collect::<Vec<_>>();
    let paths = repos
        .iter()
        .map(|repo| repo.path().canonicalize().unwrap())
        .collect::<Vec<_>>();
    settings::write_setting(
        "recent_repos",
        Some(
            &paths
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\u{1f}"),
        ),
    );
    let (app, window) = mount(cx, &paths[0]);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |app, cx| app.open_home_tab(w, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
    focus_recent(cx, &app, window);
    key(cx, window, "home");
    for _ in 0..7 {
        key(cx, window, "down");
    }
    key(cx, window, "shift-f10");
    key(cx, window, "enter");
    assert_eq!(tabs::recent_repos().len(), 11);
    assert_eq!(
        cx.update_window(window, |_, w, cx| app
            .read(cx)
            .list_row_focused_for_e2e("home-recent-list", w))
            .unwrap(),
        Some(paths[8].to_string_lossy().into_owned()),
        "removing the eighth of twelve recent rows focuses the new eighth row"
    );
    key(cx, window, "end");
    key(cx, window, "shift-f10");
    key(cx, window, "enter");
    assert_eq!(
        cx.update_window(window, |_, w, cx| app
            .read(cx)
            .list_row_focused_for_e2e("home-recent-list", w))
            .unwrap(),
        Some(paths[10].to_string_lossy().into_owned()),
        "removing the tail focuses its predecessor"
    );
    unmount(cx, app, window);
}

fn assert_open_folder_focused(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    crate::native_ax::activate(cx);
    draw(cx, window);
    let tree = crate::native_ax::tree(cx, window);
    let focused = tree["gpui_focus"]
        .as_str()
        .expect("a control retains focus");
    assert_eq!(
        tree["nodes"][focused]["element_id"], "Name(\"home-open-folder\")",
        "removing the last recent row focuses Open Folder"
    );
    draw(cx, window);
    let tree = crate::native_ax::tree(cx, window);
    let focused = tree["gpui_focus"].as_str().unwrap();
    assert_eq!(
        tree["nodes"][focused]["element_id"], "Name(\"home-open-folder\")",
        "Open Folder focus survives the next reconciliation frame"
    );
}

pub fn scenario_home_recent_remove(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang"]);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    for lang in [Lang::En, Lang::Ja] {
        middle_removal(cx, lang);
        let first = build_fixture();
        let second = build_fixture();
        let a = first.path().canonicalize().unwrap();
        let b = second.path().canonicalize().unwrap();
        let before_a = crate::macos::repo_fingerprint(&a);
        let before_b = crate::macos::repo_fingerprint(&b);
        settings::write_setting("recent_repos", Some(""));
        tabs::record_recent_repo(&a);
        tabs::record_recent_repo(&b);
        let (app, window) = mount(cx, &a);
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        cx.update_window(window, |_, w, cx| {
            app.update(cx, |app, cx| app.open_home_tab(w, cx))
        })
        .unwrap();
        cx.run_until_parked();
        draw(cx, window);
        let sessions = cx.read(|cx| {
            app.read(cx)
                .tabs
                .iter()
                .map(|t| (t.session, t.path.clone()))
                .collect::<Vec<_>>()
        });
        let active = cx.read(|cx| app.read(cx).active_tab);
        let row = e2e::control_bounds(window.window_id(), "home-recent-0").unwrap();
        cx.simulate_event(
            window,
            MouseDownEvent {
                button: MouseButton::Right,
                position: row.center(),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            },
        );
        draw(cx, window);
        assert!(
            e2e::control_bounds(window.window_id(), "home-recent-menu-item-0-0").is_some(),
            "Recent row right-click must expose Remove from Recently Opened"
        );
        crate::native_ax::activate(cx);
        draw(cx, window);
        let label = match lang {
            Lang::En => "Remove from Recently Opened",
            Lang::Ja => "最近使った項目から取り除く",
        };
        let node = crate::native_ax::node(
            cx,
            window,
            gpui::ElementId::Name("home-recent-menu-item-0-0".into()),
        );
        assert_eq!(node["aria"]["role"], "MenuItem");
        assert_eq!(node["aria"]["label"], label);
        assert!(node["aria"]["on_action"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "Click"));
        crate::app_conflict::click_control(cx, window, "home-recent-menu-item-0-0");
        draw(cx, window);
        assert_eq!(tabs::recent_repos(), vec![a.clone()]);
        focus_recent(cx, &app, window);
        key(cx, window, "shift-f10");
        e2e::clear_control_bounds(window.window_id(), "home-recent-menu-item-0-0");
        draw(cx, window);
        assert!(e2e::control_bounds(window.window_id(), "home-recent-menu-item-0-0").is_some());
        key(cx, window, "escape");
        assert!(
            cx.update_window(window, |_, w, cx| app
                .read(cx)
                .list_row_focused_for_e2e("home-recent-list", w)
                .is_some())
                .unwrap(),
            "Escape returns to the recent row"
        );
        key(cx, window, "shift-f10");
        key(cx, window, "enter");
        assert!(tabs::recent_repos().is_empty());
        assert_open_folder_focused(cx, window);
        assert_eq!(settings::read_setting("recent_repos").as_deref(), Some(""));
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.home_in_front());
            assert_eq!(app.active_tab, active);
            assert_eq!(
                app.tabs
                    .iter()
                    .map(|t| (t.session, t.path.clone()))
                    .collect::<Vec<_>>(),
                sessions
            );
        });
        settings::flush();
        let disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(settings::settings_path().unwrap()).unwrap())
                .unwrap();
        assert_eq!(
            disk["recent_repos"], "",
            "removal reaches persisted storage"
        );
        unmount(cx, app, window);
        let (app, window) = mount(cx, &a);
        cx.update_window(window, |_, w, cx| {
            app.update(cx, |app, cx| app.open_home_tab(w, cx))
        })
        .unwrap();
        cx.run_until_parked();
        draw(cx, window);
        assert!(
            tabs::recent_repos().is_empty(),
            "new window does not restore removed recent paths"
        );
        assert!(
            e2e::control_bounds(window.window_id(), "home-open-folder").is_some(),
            "empty history keeps Open Folder reachable"
        );
        assert!(a.is_dir() && b.is_dir());
        assert_eq!(crate::macos::repo_fingerprint(&a), before_a);
        assert_eq!(crate::macos::repo_fingerprint(&b), before_b);
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS home_recent_remove");
}
