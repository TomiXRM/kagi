//! #1528: user graph selection activates Changes only in compact windows.
use crate::macos::{build_fixture, git, mount_state, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab, KagiApp};
use kagi_ui_core::{i18n, theme};

/// #1570: dispatch must publish the pane before reading its repository.
pub fn scenario_compact_wip_first_frame(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    for n in 0..200 {
        std::fs::write(
            fixture.path().join(format!("change-{n:03}.txt")),
            "change\n",
        )
        .unwrap();
    }
    let before = crate::macos::repo_fingerprint(fixture.path());
    for width in [900., 1200.] {
        let state = e2e::app_state(fixture.path()).unwrap();
        theme::set_zoom(1.);
        theme::set_active("apple-light");
        cx.update(theme::sync_gpui_component_theme);
        let (app, win) = mount_state(cx, state);
        resize(cx, win, width);
        let (hold, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_commit_panel_read_for_e2e(hold);
        let (probe_hold, probe_release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_smart_probe_for_e2e(probe_hold);
        app.update(cx, |app, _| app.smart_commit.llm_enabled = true);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.open_commit_panel(window, cx);
                let panel = app
                    .ui()
                    .commit_panel
                    .as_ref()
                    .expect("pane attaches immediately");
                assert!(
                    panel.read(cx).state.unstaged.is_empty(),
                    "WIP dispatch must return before the first repository read"
                );
                assert!(app.ui().commit_panel_open);
                if kagi_domain::compact::is_compact(
                    width,
                    theme::rem_size_px(),
                    theme::compact_threshold_rem(),
                ) {
                    assert_eq!(app.bottom_tab, BottomTab::Changes);
                    assert!(app.bottom_panel_open);
                }
            });
        })
        .unwrap();
        settle(cx, win);
        assert!(
            e2e::control_bounds(win.window_id(), "cp-status-loading").is_some(),
            "the pane paints its placeholder while the real read is held"
        );
        if kagi_domain::compact::is_compact(
            width,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        ) {
            assert!(e2e::control_bounds(win.window_id(), "compact-composer").is_some());
        }
        release.send(());
        settle(cx, win);
        assert!(cx.read(|cx| app.read(cx).smart_commit.detecting));
        assert_eq!(
            e2e::commit_panel_labels::accessible_description("cp-smart-suggest").as_deref(),
            Some(i18n::Msg::SmartDetecting.t())
        );
        probe_release.send(());
        settle(cx, win);
        assert!(!cx.read(|cx| app.read(cx).smart_commit.detecting));
        cx.read(|cx| {
            let panel = app.read(cx).ui().commit_panel.as_ref().unwrap().read(cx);
            assert_eq!(panel.state.unstaged.len(), 200);
        });
        let marker = if kagi_domain::compact::is_compact(
            width,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        ) {
            "compact-changes-unstaged"
        } else {
            "right-pane-clip"
        };
        assert!(e2e::control_bounds(win.window_id(), marker).is_some());
        let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
        let title = cx.read(|cx| panel.read(cx).title_input.clone().unwrap());
        let (hold, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_commit_panel_read_for_e2e(hold);
        cx.update_window(win, |_, window, cx| {
            title.update(cx, |input, cx| input.set_value("unsaved draft", window, cx));
            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        })
        .unwrap();
        settle(cx, win);
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().commit_panel.clone()),
            Some(panel.clone()),
            "warm open retains the message owner"
        );
        // A newer refresh must invalidate the held opening read, including
        // direct post-write/watcher refreshes on the same panel entity.
        panel.update(cx, |view, cx| {
            view.reload_status(fixture.path());
            view.state.unstaged.reverse();
            cx.notify();
        });
        let first = cx.read(|cx| panel.read(cx).state.unstaged[0].path.clone());
        let displayed_digest = cx.read(|cx| panel.read(cx).state.displayed_index_digest.clone());
        assert!(displayed_digest.is_some());
        release.send(());
        settle(cx, win);
        cx.read(|cx| {
            assert_eq!(panel.read(cx).state.unstaged[0].path, first);
            assert_eq!(
                panel.read(cx).state.displayed_index_digest,
                displayed_digest
            );
            assert_eq!(panel.read(cx).title_input.as_ref(), Some(&title));
            assert_eq!(title.read(cx).value().as_ref(), "unsaved draft");
        });
        let (hold, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_commit_panel_read_for_e2e(hold);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        })
        .unwrap();
        settle(cx, win);
        app.update(cx, |app, _| {
            app.reads.invalidate_all();
        });
        release.send(());
        settle(cx, win);
        assert_eq!(
            cx.read(|cx| panel.read(cx).state.unstaged.len()),
            200,
            "an invalidated opening must reread even when no reload follows"
        );
        drop(title);
        drop(panel);
        unmount(cx, app, win);
    }
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
}

pub fn scenario_compact_wip_probe_superseded(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("staged.txt"), "staged\n").unwrap();
    git(fixture.path(), &["add", "staged.txt"]);
    let (app, win) = mount_state(cx, e2e::app_state(fixture.path()).unwrap());
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    let (probe_hold, probe_release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_commit_panel_read_for_e2e(hold);
    KagiApp::hold_next_smart_probe_for_e2e(probe_hold);
    app.update(cx, |app, _| app.smart_commit.llm_enabled = true);
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
    panel.update(cx, |panel, cx| {
        panel.reload_status(fixture.path());
        cx.notify();
    });
    release.send(());
    settle(cx, win);
    assert!(
        cx.read(|cx| app.read(cx).smart_commit.detecting),
        "a superseded opening still starts provider discovery"
    );
    assert_eq!(
        e2e::commit_panel_labels::accessible_description("cp-smart-suggest").as_deref(),
        Some(i18n::Msg::SmartDetecting.t())
    );
    probe_release.send(());
    settle(cx, win);
    assert!(!cx.read(|cx| app.read(cx).smart_commit.detecting));
    assert!(e2e::control_bounds(win.window_id(), "cp-smart-suggest").is_some());
    drop(panel);
    unmount(cx, app, win);
}

pub fn scenario_compact_wip_amend_loading(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("staged.txt"), "staged\n").unwrap();
    git(fixture.path(), &["add", "staged.txt"]);
    let before = crate::macos::repo_fingerprint(fixture.path());
    for (width, lang, reason) in [
        (900., i18n::Lang::En, "Amend: waiting for repository status"),
        (1200., i18n::Lang::Ja, "Amend: リポジトリの状態を読み込み中"),
    ] {
        i18n::set_lang(lang);
        let (app, win) = mount_state(cx, e2e::app_state(fixture.path()).unwrap());
        resize(cx, win, width);
        let (hold, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_commit_panel_read_for_e2e(hold);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        })
        .unwrap();
        let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
        cx.update_window(win, |_, window, cx| {
            let title = panel.read(cx).title_input.clone().unwrap();
            title.update(cx, |input, cx| {
                input.set_value("amended subject", window, cx)
            });
        })
        .unwrap();
        settle(cx, win);
        let description = e2e::commit_panel_labels::accessible_description("cp-amend-btn");
        let amend = e2e::control_bounds(win.window_id(), "cp-amend-btn").unwrap();
        cx.simulate_click(win, amend.center(), Modifiers::none());
        settle(cx, win);
        assert!(cx.read(|cx| app.read(cx).amend_modal().is_none()));
        app.update(cx, |app, cx| {
            app.commit_panel_amend(app.active_session().unwrap(), cx);
        });
        settle(cx, win);
        assert_eq!(
            (
                cx.read(|cx| app.read(cx).amend_modal().is_some()),
                description.as_deref()
            ),
            (false, Some(reason)),
            "loading must block both amend dispatch and the rendered control"
        );
        release.send(());
        settle(cx, win);
        app.update(cx, |app, cx| {
            app.commit_panel_amend(app.active_session().unwrap(), cx);
        });
        settle(cx, win);
        assert_eq!(
            cx.read(|cx| app.read(cx).amend_modal().unwrap().mode),
            kagi_git::ops::AmendMode::Both
        );
        app.update(cx, |app, _| app.cancel_amend_modal());
        drop(panel);
        unmount(cx, app, win);
    }
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
}

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
            resize(
                cx,
                win,
                (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
            );
            click(cx, win, "tab-terminal");
            click(cx, win, "graph-row-0");
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                BottomTab::Terminal,
                "first integral desktop width preserves pointer selection behavior"
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

pub(crate) fn assert_changes(
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

pub(crate) fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    for id in [
        "compact-inspector-details",
        "compact-inspector-files",
        "inspector-file-0",
        "compact-changes-unstaged",
        "compact-changes-staged",
        "bottom-panel",
        "right-pane-clip",
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
pub(crate) fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, width: f32) {
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
pub(crate) fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let bounds = e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("missing {id}"));
    cx.simulate_mouse_move(win, bounds.center(), None, Modifiers::none());
    cx.simulate_click(win, bounds.center(), Modifiers::none());
    settle(cx, win);
}
pub(crate) fn focus_graph(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) {
    let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
    cx.update_window(win, |_, window, cx| root.focus(window, cx))
        .unwrap();
    settle(cx, win);
}
pub(crate) fn key(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
    settle(cx, win);
}
