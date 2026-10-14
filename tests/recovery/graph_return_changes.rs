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
                let compact = width < 1000.;
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
