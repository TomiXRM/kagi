//! #1167: the commit composer must not displace its primary actions.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{
    point, px, size, Focusable, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestAppContext,
};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_commit_panel_viewport(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    for (width, height, zoom) in [
        (1392., 883., 1.),
        (1000., 660., 1.67),
        (1000., 560., 1.67),
        (940., 660., 1.67),
        (940., 560., 1.67),
    ] {
        for locale in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            if width < 1000. {
                // The compact entry is the real WIP row, absent in this clean
                // fixture. Leave the wide golden fixture completely unchanged.
                std::fs::write(fixture.path().join("compact-wip.txt"), "viewport WIP\n").unwrap();
            }
            theme::set_zoom(zoom);
            let state = e2e::app_state(fixture.path()).unwrap();
            let captured = Rc::new(RefCell::new(None));
            let output = captured.clone();
            let window =
                crate::macos::open_offscreen(cx, size(px(width), px(height)), move |window, cx| {
                    e2e::mount_root(state, window, cx, &output)
                });
            let app = captured.borrow().clone().unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                let panel = app.read(cx).ui().commit_panel.clone().unwrap();
                let body = panel.read(cx).body_input.clone().unwrap();
                body.update(cx, |input, cx| {
                    input.set_value("body line\n".repeat(30), window, cx)
                });
            })
            .unwrap();
            for _ in 0..4 {
                cx.run_until_parked();
                draw(cx, window.into(), (width, height));
            }
            for name in [
                "cp-files",
                "cp-body",
                "cp-smart-suggest",
                "cp-commit-action",
            ] {
                let bounds = e2e::control_bounds(window.window_id(), name)
                    .unwrap_or_else(|| panic!("missing {name}"));
                eprintln!(
                    "commit-panel snapshot {locale}/{width}x{height}@{zoom} {name}: {bounds:?}"
                );
                assert!(
                    bounds.size.height > px(0.),
                    "{name} must remain usable: {bounds:?}"
                );
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= px(height),
                    "{locale}/{width}x{height}@{zoom}: {name} outside viewport: {bounds:?}"
                );
                assert!(
                    bounds.left() >= px(0.) && bounds.right() <= px(width),
                    "{name} outside viewport width: {bounds:?}"
                );
                if zoom == 1. {
                    let expected_height = match name {
                        // #1462 restores the 36px titlebar above the 52px toolbar.
                        // Only the flexible file list gives up viewport height;
                        // the fixed AutoGrow-era composer/action sizes below stay.
                        "cp-files" => 408.,
                        "cp-body" => 257.,
                        "cp-smart-suggest" => 21.5,
                        "cp-commit-action" => 25.,
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        bounds.size.height,
                        px(expected_height),
                        "desktop height must match the titlebar-plus-toolbar layout"
                    );
                }
            }
            let files = e2e::control_bounds(window.window_id(), "cp-files").unwrap();
            if zoom == 1. {
                assert_eq!(files.size.width, px(324.), "recorded desktop panel width");
            }
            let composer = e2e::control_bounds(window.window_id(), "cp-body").unwrap();
            let generate = e2e::control_bounds(window.window_id(), "cp-smart-suggest").unwrap();
            let commit = e2e::control_bounds(window.window_id(), "cp-commit-action").unwrap();
            if width >= 1000. {
                assert!(
                    e2e::control_bounds(window.window_id(), "compact-composer").is_none(),
                    "desktop viewport must exercise the original Commit Panel"
                );
                assert!(files.bottom() <= composer.top());
            } else {
                let popup = e2e::control_bounds(window.window_id(), "compact-composer").unwrap();
                crate::recovery_layout::contained(popup, composer, "compact body");
                crate::recovery_layout::contained(popup, commit, "compact Commit");
                for column in ["compact-changes-unstaged", "compact-changes-staged"] {
                    crate::recovery_layout::contained(
                        files,
                        e2e::control_bounds(window.window_id(), column).unwrap(),
                        column,
                    );
                }
            }
            crate::recovery_layout::contained(
                composer,
                generate,
                "Generate remains within body action row",
            );
            assert!(
                composer.bottom() <= commit.top(),
                "body/actions must never overpaint Commit"
            );
            let panel = cx
                .read(|cx| app.read(cx).ui().commit_panel.clone())
                .unwrap();
            let body = cx.read(|cx| panel.read(cx).body_input.clone()).unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                body.update(cx, |input, cx| {
                    input.set_cursor_position(
                        gpui_component::input::Position::default(),
                        window,
                        cx,
                    );
                    input.set_scroll_offset(point(px(0.), px(0.)), cx);
                });
            })
            .unwrap();
            draw(cx, window.into(), (width, height));
            let before = cx.read(|cx| body.read(cx).scroll_offset().y);
            // Added titlebar space can bring the icon row above the former
            // fixed +24px point. Wheel over the actual editable viewport.
            let viewport = e2e::control_bounds(window.window_id(), "cp-body-input").unwrap();
            cx.simulate_event(
                window.into(),
                ScrollWheelEvent {
                    position: viewport.center(),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
                    touch_phase: TouchPhase::Moved,
                    ..Default::default()
                },
            );
            draw(cx, window.into(), (width, height));
            assert!(
                cx.read(|cx| body.read(cx).scroll_offset().y) < before,
                "wheel must scroll the body internally"
            );
            cx.update_window(window.into(), |_, window, cx| {
                window.focus(&body.read(cx).focus_handle(cx), cx);
            })
            .unwrap();
            cx.simulate_keystrokes(window.into(), "cmd-end");
            cx.simulate_input(window.into(), "editable tail");
            draw(cx, window.into(), (width, height));
            assert!(
                cx.read(|cx| body.read(cx).value().contains("editable tail")),
                "the bounded editor remains editable"
            );
            if zoom == 1. {
                let title = cx.read(|cx| panel.read(cx).title_input.clone()).unwrap();
                for (label, text, expected_rows) in [
                    ("three-lines", vec!["body"; 3].join("\n"), 3),
                    ("eleven-lines", vec!["body"; 11].join("\n"), 11),
                    ("wrapped-lines", vec!["a".repeat(72); 3].join("\n"), 6),
                ] {
                    cx.update_window(window.into(), |_, window, cx| {
                        title.update(cx, |input, cx| input.set_value("subject", window, cx));
                        body.update(cx, |input, cx| input.set_value(text.clone(), window, cx));
                    })
                    .unwrap();
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window.into(), (width, height));
                    }
                    assert_eq!(
                        cx.read(|cx| body.read(cx).wrap_row_count()),
                        expected_rows,
                        "{label} visual rows"
                    );
                    let bounds = e2e::control_bounds(window.window_id(), "cp-body").unwrap();
                    // Pre-fix AutoGrow: 18px per visual row + 16px Input padding
                    // + 25px action row/borders, with the same pinned controls.
                    assert_eq!(
                        bounds.size.height,
                        px(expected_rows as f32 * 18. + 41.),
                        "{locale}/{label}: AutoGrow-era desktop body height"
                    );
                    eprintln!("commit-panel desktop snapshot {locale}/{label}: {bounds:?}");
                }
                drop(title);
            }
            drop(body);
            drop(panel);
            unmount(cx, app, window.into());
        }
    }
    right_edge_matrix(cx);
    eprintln!("[gui-e2e] PASS commit_panel_viewport");
}

fn right_edge_matrix(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let path = format!("{}.txt", "long-file-name-".repeat(12));
    std::fs::write(fixture.path().join(&path), "staged\n").unwrap();
    git(fixture.path(), &["add", "--", &path]);
    std::fs::write(fixture.path().join(&path), "unstaged\n").unwrap();
    for (width, height, zoom) in [
        (1392., 883., 1.),
        (1000., 660., 1.67),
        (1000., 560., 1.67),
        (940., 660., 1.67),
    ] {
        for locale in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            theme::set_zoom(zoom);
            let state = e2e::app_state(fixture.path()).unwrap();
            let captured = Rc::new(RefCell::new(None));
            let output = captured.clone();
            let window =
                crate::macos::open_offscreen(cx, size(px(width), px(height)), move |window, cx| {
                    e2e::mount_root(state, window, cx, &output)
                });
            let app = captured.borrow().clone().unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| {
                    // A user-widened panel must adapt when zoom/viewport changes.
                    if zoom > 1. {
                        app.panel_width = 480.;
                    }
                    app.open_commit_panel(window, cx);
                });
            })
            .unwrap();
            let panel = cx
                .read(|cx| app.read(cx).ui().commit_panel.clone())
                .unwrap();
            for tree in [false, true] {
                panel.update(cx, |panel, cx| {
                    panel.state.tree_view = tree;
                    cx.notify();
                });
                for _ in 0..4 {
                    cx.run_until_parked();
                    draw(cx, window.into(), (width, height));
                }
                for name in [
                    "cp-files",
                    "cp-view-tree",
                    "cp-row-unstage",
                    "cp-row-stage",
                    "cp-amend-btn",
                ] {
                    let bounds = e2e::control_bounds(window.window_id(), name)
                        .unwrap_or_else(|| panic!("missing {name}"));
                    eprintln!("commit-panel right edge {locale}/{width}x{height}@{zoom}/tree={tree} {name}: {bounds:?}");
                    assert!(
                        bounds.left() >= px(0.) && bounds.right() <= px(width),
                        "{name} outside window: {bounds:?}"
                    );
                    if name == "cp-files" && zoom == 1. {
                        assert_eq!(bounds.size.width, px(324.), "recorded desktop panel width");
                    }
                }
            }
            drop(panel);
            unmount(cx, app, window.into());
        }
    }
}
