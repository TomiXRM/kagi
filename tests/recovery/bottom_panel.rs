//! Native geometry regression for the bottom panel inside nested workspaces.
//! PR/Issues own a navigator, and Editor owns a file tree and hunks pane;
//! none of those side panes may stop above the host's bottom panel.

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, theme};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for name in [
        "bottom-panel",
        "pr-mode-left-pane",
        "pr-mode-center-pane",
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "editor-workspace-slot",
    ] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .expect("draw nested workspace");
}

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} was not painted"))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 2.0
}

fn right(rect: Bounds<Pixels>) -> f32 {
    f32::from(rect.origin.x + rect.size.width)
}

fn bottom(rect: Bounds<Pixels>) -> f32 {
    f32::from(rect.origin.y + rect.size.height)
}

fn assert_nested_center(
    window: AnyWindowHandle,
    left_name: &str,
    center_name: &str,
) -> Bounds<Pixels> {
    let panel = bounds(window, "bottom-panel");
    let left = bounds(window, left_name);
    let center = bounds(window, center_name);
    assert!(
        near(bottom(left), bottom(panel)),
        "{left_name} stops above the bottom panel: {left:?} {panel:?}"
    );
    assert!(
        near(f32::from(panel.origin.x), f32::from(center.origin.x))
            && near(right(panel), right(center)),
        "panel is not beneath {center_name} alone: {panel:?} {center:?}"
    );
    assert!(
        bottom(center) <= f32::from(panel.origin.y) + 2.0,
        "{center_name} overlaps panel: {center:?} {panel:?}"
    );
    assert!(
        f32::from(panel.origin.x) >= right(left) - 2.0,
        "panel extends beneath {left_name}: {panel:?} {left:?}"
    );
    panel
}

pub fn scenario_bottom_panel_nested(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);

    // Opening PR mode closes the panel by default; explicitly reopen it to
    // exercise the center/left boundary even on the empty dashboard.
    app.update(cx, |app, cx| {
        app.show_pr_mode(cx);
        app.bottom_panel_open = true;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, window);
    assert_nested_center(window, "pr-mode-left-pane", "pr-mode-center-pane");

    app.update(cx, |app, cx| app.show_issues_mode(cx));
    cx.run_until_parked();
    draw(cx, window);
    assert_nested_center(window, "issue-mode-left-pane", "issue-mode-center-pane");

    app.update(cx, |app, cx| app.show_editor_mode(cx));
    cx.run_until_parked();
    draw(cx, window);
    let panel = bounds(window, "bottom-panel");
    let editor_root = bounds(window, "editor-workspace-slot");
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("Editor workspace opened");
    let (tree_width, hunks_width) = cx.read(|cx| {
        let view = editor.read(cx);
        (view.tree_w, view.hunks_w)
    });
    let physical_scale = theme::scaled(1.0);
    assert!(
        near(bottom(panel), bottom(editor_root)),
        "Editor pane should span the bottom panel: {panel:?} {editor_root:?}"
    );
    assert!(
        f32::from(panel.origin.x)
            >= f32::from(editor_root.origin.x) + tree_width * physical_scale - 2.0,
        "bottom panel extends under Editor file tree: {panel:?} {editor_root:?}"
    );
    assert!(
        right(panel) <= right(editor_root) - hunks_width * physical_scale + 2.0,
        "bottom panel extends under Editor hunks: {panel:?} {editor_root:?}"
    );

    // An Editor-only notification must rebuild the parent-provided panel;
    // the changed tree width should move its left edge, not make it vanish.
    editor.update(cx, |view, cx| {
        view.tree_w += 24.0;
        cx.notify();
    });
    draw(cx, window);
    let moved = bounds(window, "bottom-panel");
    assert!(
        f32::from(moved.origin.x) >= f32::from(panel.origin.x) + 24.0 * physical_scale - 2.0,
        "Editor-only rerender lost or misplaced the panel: {panel:?} {moved:?}"
    );
    assert!(
        near(right(moved), right(panel)) && near(bottom(moved), bottom(editor_root)),
        "Editor hunks and footer must stay beside the panel after resize: {moved:?}"
    );

    assert_eq!(
        before,
        repo_fingerprint(&repo),
        "layout scenario wrote to repo"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS bottom_panel_nested PR/Issues/Editor");
}

/// #1205 / #1206: manual keyboard activation and persistent selection paint.
pub fn scenario_bottom_panel_tab_hover(cx: &mut VisualTestAppContext) {
    use kagi::ui::{i18n::Lang, BottomTab};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let restore_globals = crate::recovery_layout::GlobalSettings::capture();
    struct RestoreTheme(String);
    impl Drop for RestoreTheme {
        fn drop(&mut self) {
            theme::set_active(&self.0);
        }
    }
    let restore_theme = RestoreTheme(theme::theme().slug.to_string());
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    let paint = |cx: &mut VisualTestAppContext, id: &str| {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
            let rect = e2e::control_bounds(window.window_handle().window_id(), id)
                .expect("bottom tab measured");
            let image = window.render_to_image().expect("native tab capture");
            let scale = window.scale_factor();
            image
                .get_pixel(
                    ((f32::from(rect.left()) + theme::scaled(8.)) * scale) as u32,
                    ((f32::from(rect.top()) + theme::scaled(5.)) * scale) as u32,
                )
                .0
        })
        .unwrap()
    };
    for slug in ["apple-light", "apple-dark"] {
        for zoom in [1.0, 1.67] {
            for lang in [Lang::En, Lang::Ja] {
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    theme::set_zoom(zoom);
                    app.set_lang(lang, cx);
                    app.bottom_tab = BottomTab::OperationLog;
                    cx.notify();
                });
                cx.run_until_parked();
                for active in [false, true] {
                    app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
                    cx.simulate_mouse_move(
                        window,
                        gpui::point(gpui::px(0.), gpui::px(0.)),
                        None,
                        gpui::Modifiers::none(),
                    );
                    let rest = paint(cx, "tab-oplog");
                    crate::app_conflict::click_control(cx, window, "tab-oplog");
                    assert_eq!(
                        paint(cx, "tab-oplog"),
                        rest,
                        "selection wins over hover: {slug}/{zoom}/{lang:?}/{active}"
                    );
                    if slug == "apple-light" && !active {
                        assert_eq!(&rest[..3], &[211, 211, 211]);
                    }
                    let other = bounds(window, "tab-activity");
                    cx.simulate_mouse_move(window, other.center(), None, gpui::Modifiers::none());
                    assert_eq!(
                        paint(cx, "tab-oplog"),
                        rest,
                        "unselected hover cannot erase selection"
                    );
                    assert_eq!(
                        cx.read(|cx| app.read(cx).bottom_tab),
                        BottomTab::OperationLog
                    );
                }
                let attrs = cx.read(|cx| app.read(cx).bottom_tab_attributes_for_e2e());
                let labels = if lang == Lang::En {
                    ["Operation Log", "Terminal", "Activity"]
                } else {
                    ["操作ログ", "ターミナル", "アクティビティ"]
                };
                assert_eq!(attrs.len(), 3);
                assert_eq!(attrs.iter().filter(|a| a.2).count(), 1);
                assert_eq!(attrs.iter().filter(|a| a.3).count(), 1);
                for (attr, label) in attrs.iter().zip(labels) {
                    assert_eq!(attr.0, gpui::Role::Tab);
                    assert_eq!(attr.1, label);
                }
                for id in ["tab-oplog", "tab-terminal", "tab-activity"] {
                    let cell = bounds(window, id);
                    let label = bounds(window, &format!("{id}-label"));
                    assert!(label.left() >= cell.left() && label.right() <= cell.right());
                    assert!(label.top() >= cell.top() && label.bottom() <= cell.bottom());
                }
                let reached = cx
                    .update_window(window, |_, window, cx| {
                        let root = app.read(cx).root_focus.clone().unwrap();
                        root.focus(window, cx);
                        window.draw(cx).clear();
                        (0..100).find_map(|_| {
                            window.focus_next(cx);
                            window.draw(cx).clear();
                            app.read(cx).bottom_tab_focused_for_e2e(window)
                        })
                    })
                    .unwrap();
                assert_eq!(reached, Some(0), "Tab reaches the selected pane");
                let spawns = cx.read(|cx| {
                    app.read(cx)
                        .ui()
                        .terminal_session
                        .as_ref()
                        .map_or(0, |session| session.spawns)
                });
                crate::keyboard_nav::keys(cx, window, "right");
                assert_eq!(
                    cx.read(|cx| app.read(cx).bottom_tab),
                    BottomTab::OperationLog,
                    "arrow focus never activates Terminal"
                );
                assert_eq!(
                    cx.read(|cx| app
                        .read(cx)
                        .ui()
                        .terminal_session
                        .as_ref()
                        .map_or(0, |session| session.spawns)),
                    spawns,
                    "arrow focus never starts a shell"
                );
                crate::keyboard_nav::keys(cx, window, "enter");
                assert_eq!(cx.read(|cx| app.read(cx).bottom_tab), BottomTab::Terminal);
                cx.simulate_keystrokes(window, "f19");
                cx.update_window(window, |_, window, cx| {
                    window.draw(cx).clear();
                    let rect = bounds(window.window_handle(), "tab-terminal");
                    let image = window.render_to_image().expect("native focus-ring capture");
                    let scale = window.scale_factor();
                    let ring = image
                        .get_pixel(
                            (f32::from(rect.center().x) * scale) as u32,
                            ((f32::from(rect.top()) - 1.) * scale) as u32,
                        )
                        .0;
                    let accent = theme::theme().ui_accent();
                    assert_eq!(
                        &ring[..3],
                        &[
                            ((accent >> 16) & 255) as u8,
                            ((accent >> 8) & 255) as u8,
                            (accent & 255) as u8,
                        ],
                        "focus ring is independent of selected fill"
                    );
                })
                .unwrap();
                cx.simulate_mouse_move(
                    window,
                    gpui::point(gpui::px(0.), gpui::px(0.)),
                    None,
                    gpui::Modifiers::none(),
                );
                let terminal_rest = paint(cx, "tab-terminal");
                let terminal = bounds(window, "tab-terminal");
                cx.simulate_mouse_move(window, terminal.center(), None, gpui::Modifiers::none());
                assert_eq!(
                    paint(cx, "tab-terminal"),
                    terminal_rest,
                    "selected Terminal retains its fill under hover"
                );
                crate::keyboard_nav::keys(cx, window, "right space");
                assert_eq!(cx.read(|cx| app.read(cx).bottom_tab), BottomTab::Activity);
                crate::keyboard_nav::keys(cx, window, "left");
                assert_eq!(
                    cx.update_window(window, |_, window, cx| app
                        .read(cx)
                        .bottom_tab_focused_for_e2e(window))
                        .unwrap(),
                    Some(1)
                );
                crate::keyboard_nav::keys(cx, window, "home space end");
                assert_eq!(
                    cx.read(|cx| app.read(cx).bottom_tab),
                    BottomTab::OperationLog
                );
                assert_eq!(
                    cx.update_window(window, |_, window, cx| app
                        .read(cx)
                        .bottom_tab_focused_for_e2e(window))
                        .unwrap(),
                    Some(2)
                );
                for forward in [true, false] {
                    cx.update_window(window, |_, window, cx| {
                        window.draw(cx).clear();
                        if forward {
                            window.focus_next(cx);
                        } else {
                            window.focus_prev(cx);
                        }
                        window.draw(cx).clear();
                        assert_eq!(
                            app.read(cx).bottom_tab_focused_for_e2e(window),
                            None,
                            "one Tab leaves the strip"
                        );
                    })
                    .unwrap();
                    // Re-enter by traversal to the selected pane.
                    cx.update_window(window, |_, window, cx| {
                        let root = app.read(cx).root_focus.clone().unwrap();
                        root.focus(window, cx);
                        window.draw(cx).clear();
                        assert_eq!(
                            (0..100).find_map(|_| {
                                window.focus_next(cx);
                                window.draw(cx).clear();
                                app.read(cx).bottom_tab_focused_for_e2e(window)
                            }),
                            Some(0)
                        );
                    })
                    .unwrap();
                }
                app.update(cx, |app, cx| {
                    app.bottom_panel_open = false;
                    cx.notify();
                });
                cx.run_until_parked();
                draw(cx, window);
                assert_eq!(
                    cx.update_window(window, |_, window, cx| app
                        .read(cx)
                        .bottom_tab_focused_for_e2e(window))
                        .unwrap(),
                    None,
                    "hidden pane yields focus"
                );
                app.update(cx, |app, cx| {
                    app.bottom_panel_open = true;
                    cx.notify();
                });
                cx.run_until_parked();
                draw(cx, window);
                crate::app_conflict::click_control(cx, window, "tab-terminal");
                assert!(
                    cx.update_window(window, |_, window, cx| {
                        app.read(cx).terminal_focused_for_e2e(window, cx)
                    })
                    .unwrap(),
                    "pointer activation hands text input to the PTY"
                );
            }
        }
    }
    assert_eq!(before, repo_fingerprint(fixture.path()));
    app.update(cx, |app, cx| app.set_theme(&restore_theme.0, cx));
    drop(restore_theme);
    drop(restore_globals);
    cx.update(theme::sync_gpui_component_theme);
    unmount(cx, app, window);
}
