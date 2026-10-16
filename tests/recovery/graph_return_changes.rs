//! #1543: returning from PRs/Issues preserves deactivation until real selection.
use crate::compact_select_changes::{assert_changes, click, focus_graph, key, resize, settle};
use crate::macos::{build_fixture, git, mount_state, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab, KagiApp};
use kagi_ui_core::{i18n, theme};

pub fn scenario_graph_return_reopens_changes(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduce_before = theme::reduce_motion();
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _offline = crate::gui_isolation::OfflineNetwork::install();
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
    let before = repo_fingerprint(fixture.path());
    for locale in ["en", "ja"] {
        for zoom in [1., 1.67] {
            for width in [900., 1200.] {
                std::env::set_var("KAGI_LANG", locale);
                i18n::init_lang();
                let state = e2e::app_state(fixture.path()).unwrap();
                theme::set_zoom(zoom);
                theme::set_active("apple-light");
                theme::set_reduce_motion(true);
                cx.update(theme::sync_gpui_component_theme);
                let (app, win) = mount_state(cx, state);
                resize(cx, win, width);
                let compact = kagi_domain::compact::is_compact(
                    width,
                    theme::rem_size_px(),
                    theme::compact_threshold_rem(),
                );
                // Both departure routes must retain a closed bottom pane on return.
                for issues in [false, true] {
                    app.update(cx, |app, cx| {
                        // PR entry closes an open pane. Issues retains the
                        // already-closed state from the previous Graph leg.
                        if !issues {
                            app.bottom_panel_open = true;
                        }
                        app.bottom_tab = BottomTab::OperationLog;
                        if issues {
                            app.show_issues_mode(cx);
                        } else {
                            app.show_pr_mode(cx);
                        }
                    });
                    settle(cx, win);
                    assert_closed(cx, &app, win, "PRs/Issues entry");
                    app.update(cx, |app, cx| app.show_graph_mode(cx));
                    settle(cx, win);
                    assert_closed(cx, &app, win, "Graph return before selection");
                    click(cx, win, "graph-row-0");
                    assert_projection(cx, &app, win, 0, compact, locale, zoom);
                    focus_graph(cx, &app, win);
                    key(cx, win, "down");
                    assert_projection(cx, &app, win, 1, compact, locale, zoom);
                    key(cx, win, "up");
                    assert_projection(cx, &app, win, 0, compact, locale, zoom);
                    eprintln!("[gui-e2e] graph_return selection PASS {locale}/{zoom}/{width}/issues={issues}");
                    // Existing selection-clear is a separate, higher Escape layer.
                    key(cx, win, "escape");
                    assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
                    assert_eq!(
                        cx.read(|cx| app.read(cx).bottom_panel_open),
                        compact,
                        "selection-clear Escape must not also close the bottom pane"
                    );
                    key(cx, win, "escape");
                    assert_closed(cx, &app, win, "final Graph Escape");
                    // Keyboard selection with no selected row reopens the compact pane.
                    key(cx, win, "down");
                    assert_projection(cx, &app, win, 0, compact, locale, zoom);
                    // Closing via Cmd-J retains selection; same-row pointer toggle
                    // and clamped keyboard navigation are not fresh selections.
                    if compact {
                        key(cx, win, "cmd-j");
                    }
                    assert_closed(cx, &app, win, "manual close with selection retained");
                    key(cx, win, "up");
                    assert_closed(cx, &app, win, "clamped same-row keyboard input");
                    click(cx, win, "graph-row-0");
                    assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
                    assert_closed(cx, &app, win, "same-row pointer toggle");
                }
                // Desktop keeps the right Inspector, but final Graph Escape can
                // close an explicitly opened Terminal/Operation Log as well.
                app.update(cx, |app, cx| {
                    app.bottom_panel_open = true;
                    cx.notify();
                });
                settle(cx, win);
                focus_graph(cx, &app, win);
                key(cx, win, "escape");
                assert_closed(cx, &app, win, "Graph Escape with no selection");
                app.update(cx, |app, cx| {
                    app.show_editor_mode(cx);
                    app.bottom_panel_open = true;
                    cx.notify();
                });
                settle(cx, win);
                focus_graph(cx, &app, win);
                key(cx, win, "escape");
                assert!(
                    cx.read(|cx| app.read(cx).bottom_panel_open),
                    "Editor Escape remains unchanged"
                );
                unmount(cx, app, win);
            }
        }
    }
    theme::set_reduce_motion(reduce_before);
    assert_eq!(
        before,
        repo_fingerprint(fixture.path()),
        "navigation never writes Git state"
    );
}

fn assert_closed(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    step: &str,
) {
    assert!(
        !cx.read(|cx| app.read(cx).bottom_panel_open),
        "{step}: bottom pane must be closed"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "bottom-panel").is_none(),
        "{step}: no bottom pane painted"
    );
}

fn assert_projection(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    row: usize,
    compact: bool,
    locale: &str,
    zoom: f32,
) {
    if compact {
        assert_changes(cx, app, win, row, locale, zoom);
    } else {
        assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(row));
        assert_eq!(
            cx.read(|cx| app.read(cx).bottom_tab),
            BottomTab::OperationLog
        );
        assert_closed(cx, app, win, "desktop selection uses right Inspector");
        assert!(e2e::control_bounds(win.window_id(), "right-pane-clip").is_some());
        assert!(e2e::control_bounds(win.window_id(), "inspector-file-0").is_some());
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
        assert!(files.rows.values().any(|(name, _)| name.ends_with(path)));
    }
}

pub fn scenario_graph_return_input_escape(cx: &mut VisualTestAppContext) {
    use gpui::Focusable as _;
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    // Only this desktop Input-Escape oracle needs the inline Commit Panel.
    // The neighboring return/selection matrices retain the default breakpoint.
    theme::set_compact_threshold_rem(40);
    let _offline = crate::gui_isolation::OfflineNetwork::install();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "unsent WIP\n").unwrap();
    for locale in ["en", "ja"] {
        for zoom in [1., 1.67] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            theme::set_zoom(zoom);
            let (app, win) = mount_state(cx, e2e::app_state(fixture.path()).unwrap());
            resize(cx, win, 1200.);
            cx.update_window(win, |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.bottom_tab = BottomTab::OperationLog;
                    app.bottom_panel_open = true;
                    app.open_commit_panel(window, cx);
                });
            })
            .unwrap();
            settle(cx, win);
            assert!(
                e2e::control_bounds(win.window_id(), "right-pane-clip").is_some(),
                "the Input-Escape oracle requires the drawn desktop Commit Panel"
            );
            assert!(e2e::control_bounds(win.window_id(), "compact-composer").is_none());
            let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
            for body in [true, false] {
                let input = cx.read(|cx| {
                    let panel = panel.read(cx);
                    if body {
                        panel.body_input.clone()
                    } else {
                        panel.title_input.clone()
                    }
                    .unwrap()
                });
                cx.update_window(win, |_, window, cx| {
                    input.focus_handle(cx).focus(window, cx)
                })
                .unwrap();
                key(cx, win, "escape");
                assert!(
                    cx.read(|cx| app.read(cx).bottom_panel_open),
                    "desktop focused Commit Panel input Escape must preserve bottom pane"
                );
                assert!(cx
                    .update_window(win, |_, window, cx| input
                        .focus_handle(cx)
                        .is_focused(window))
                    .unwrap());
                drop(input);
            }
            drop(panel);
            unmount(cx, app, win);
        }
    }
}

pub fn scenario_graph_return_bottom_focus(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _offline = crate::gui_isolation::OfflineNetwork::install();
    let fixture = build_fixture();
    for width in [900., 1200.] {
        let (app, win) = mount_state(cx, e2e::app_state(fixture.path()).unwrap());
        resize(cx, win, width);
        app.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::OperationLog;
            cx.notify();
        });
        focus_graph(cx, &app, win);
        let mut reached = false;
        for _ in 0..100 {
            key(cx, win, "tab");
            if cx
                .update_window(win, |_, window, cx| {
                    app.read(cx).bottom_tab_focused_for_e2e(window).is_some()
                })
                .unwrap()
            {
                reached = true;
                break;
            }
        }
        assert!(reached, "real Tab must enter the bottom selector");
        // Check the handoff immediately, before repaint can repair hidden focus.
        let keystroke = gpui::Keystroke::parse("escape").unwrap();
        cx.dispatch_keystroke(win, keystroke.clone());
        cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
        assert!(!cx.read(|cx| app.read(cx).bottom_panel_open));
        assert!(
            cx.update_window(win, |_, window, cx| app
                .read(cx)
                .root_focus
                .as_ref()
                .unwrap()
                .is_focused(window))
                .unwrap(),
            "bottom Escape must hand focus to Graph root before hiding its controls"
        );
        key(cx, win, "cmd-j");
        assert!(
            cx.read(|cx| app.read(cx).bottom_panel_open),
            "immediate Cmd-J remains reachable after bottom Escape"
        );
        unmount(cx, app, win);
    }
}
