//! #1533: long changed-file identities retain their distinguishing suffix.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{px, size, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    git(fixture.path(), &["config", "commit.template", "/dev/null"]);
    for suffix in ["alpha", "beta"] {
        let name =
            format!("very-long-shared-file-name-for-the-commit-panel-regression-{suffix}.txt");
        std::fs::write(fixture.path().join(name), "staged\n").unwrap();
    }
    git(fixture.path(), &["add", "."]);
    for suffix in ["alpha", "beta"] {
        let name =
            format!("very-long-shared-file-name-for-the-commit-panel-regression-{suffix}.txt");
        std::fs::write(fixture.path().join(name), "unstaged\n").unwrap();
    }
    for zoom in [1., 1.67] {
        // Keep the narrow desktop panel (and its clipping oracle) at each zoom.
        let width = if zoom == 1. { 1392. } else { 2000. };
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            for tree in [false, true] {
                theme::set_zoom(zoom);
                i18n::set_lang(lang);
                let state = e2e::app_state(fixture.path()).unwrap();
                let captured = Rc::new(RefCell::new(None));
                let output = captured.clone();
                let window = crate::macos::open_offscreen(
                    cx,
                    size(px(width), px(883.)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                );
                // Hidden native windows need the existing activation-input seam
                // for #1591's production tooltip lifetime guard.
                let _active = kagi_ui_core::tooltip::e2e::active_window(window.window_id());
                let app = captured.borrow().clone().unwrap();
                cx.update_window(window.into(), |_, window, cx| {
                    app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                    let panel = app.read(cx).ui().commit_panel.clone().unwrap();
                    panel.update(cx, |panel, cx| {
                        panel.state.tree_view = tree;
                        cx.notify();
                    });
                })
                .unwrap();
                for _ in 0..4 {
                    cx.run_until_parked();
                    draw(cx, window.into(), (width, 883.));
                }
                for section in ["us", "st"] {
                    for fi in 0..2 {
                        let id = format!("cp-{section}-file-label-{fi}");
                        let paint = e2e::control_paint(window.window_id(), &id)
                            .expect("file label painted");
                        assert!(paint.middle_ellipsis, "#1533: {id} must use native middle ellipsis, not hide alpha/beta suffixes");
                        crate::recovery_layout::contained(paint.mask, paint.bounds, &id);
                        cx.simulate_mouse_move(
                            window.into(),
                            gpui::point(px(1.), px(1.)),
                            None,
                            gpui::Modifiers::none(),
                        );
                        cx.advance_clock(std::time::Duration::from_millis(400));
                        cx.run_until_parked();
                        draw(cx, window.into(), (width, 883.));
                        e2e::text_caps::clear();
                        cx.simulate_mouse_move(
                            window.into(),
                            paint.bounds.center(),
                            None,
                            gpui::Modifiers::none(),
                        );
                        for _ in 0..3 {
                            cx.advance_clock(std::time::Duration::from_millis(400));
                            cx.run_until_parked();
                            draw(cx, window.into(), (width, 883.));
                        }
                        let suffix = if fi == 0 { "alpha" } else { "beta" };
                        let expected = format!("very-long-shared-file-name-for-the-commit-panel-regression-{suffix}.txt");
                        assert_eq!(
                            e2e::text_caps::tooltip().as_deref(),
                            Some(expected.as_str()),
                            "hover exposes complete path"
                        );
                        let tooltip =
                            e2e::control_bounds(window.window_id(), "graph-subject-tooltip")
                                .expect("full path tooltip painted");
                        crate::recovery_layout::contained(
                            gpui::Bounds::new(
                                gpui::point(px(0.), px(0.)),
                                size(px(width), px(883.)),
                            ),
                            tooltip,
                            "full path tooltip",
                        );
                    }
                }
                unmount(cx, app, window.into());
            }
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_file_middle_ellipsis");
}
