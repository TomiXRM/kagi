//! #1652: the toolbar fits without the PR sidebar opener, which #1790 moved
//! to the titlebar after the window controls.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::{contained, draw};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

const CONTROLS: &[&str] = &[
    "tb-refresh",
    "toolbar-identity",
    "tb-pull",
    "tb-push",
    "tb-branch",
    "tb-stash",
    "tb-pop",
    "tb-undo",
    "tb-redo",
    "tb-terminal",
    "tb-graph-mode",
    "tb-pr-mode",
    "tb-editor-ws",
    "tb-ecosystem",
    "tb-settings",
    "tb-update",
    "tb-overflow",
];

pub fn scenario_toolbar_pr_sidebar(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    let fixture = build_fixture();
    theme::set_compact_threshold_rem(40);
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                for display in [
                    settings::ToolbarDisplay::IconAndText,
                    settings::ToolbarDisplay::IconOnly,
                ] {
                    settings::set_toolbar_display(display);
                    let state = e2e::app_state(fixture.path()).unwrap();
                    let captured = Rc::new(RefCell::new(None));
                    let output = captured.clone();
                    let win: AnyWindowHandle = crate::macos::open_offscreen(
                        cx,
                        size(px(1392.), px(660.)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    )
                    .into();
                    let app = captured.borrow().clone().unwrap();
                    app.update(cx, |app, cx| {
                        app.view_mut().status_summary.repo_name = "stress-pr-repository".repeat(8);
                        app.view_mut().status_summary.branch = "stress-pr-branch".repeat(8);
                        crate::toolbar_overflow_priority::offer_update(app);
                        app.show_pr_mode(cx);
                    });
                    // Include many fit boundaries, not just a single roomy width.
                    for base_width in (580..=1000).step_by(8) {
                        let width = (base_width as f32 * zoom).round();
                        crate::pr_detail_adaptive::resize(cx, win, (width, 660.));
                        for _ in 0..8 {
                            cx.run_until_parked();
                            draw(cx, win, (width, 660.));
                        }
                        for id in CONTROLS.iter().chain(&["sidebar-toggle"]) {
                            e2e::clear_control_bounds(win.window_id(), id);
                        }
                        draw(cx, win, (width, 660.));
                        let strip = e2e::control_bounds(win.window_id(), "repo-tab-strip").unwrap();
                        let toggle =
                            e2e::control_bounds(win.window_id(), "sidebar-toggle").unwrap();
                        contained(strip, toggle, "sidebar-toggle");
                        let row = e2e::control_bounds(win.window_id(), "toolbar-row").unwrap();
                        let controls: Vec<_> = CONTROLS
                            .iter()
                            .filter_map(|id| {
                                let bounds = e2e::control_bounds(win.window_id(), id)?;
                                if bounds.size.width <= px(0.) {
                                    return None;
                                }
                                let bounds = if id.starts_with("tb-") && *id != "tb-overflow" {
                                    bounds.dilate(px(kagi_ui_core::keyboard_nav::RING))
                                } else {
                                    bounds
                                };
                                contained(row, bounds, id);
                                Some((id, bounds))
                            })
                            .collect();
                        for (index, (left, a)) in controls.iter().enumerate() {
                            for (right, b) in &controls[index + 1..] {
                                assert!(a.right() <= b.left() || b.right() <= a.left()
                                    || a.bottom() <= b.top() || b.bottom() <= a.top(),
                                    "{preset}/{language}/{display:?}/{width}/{zoom}: {left} overlaps {right}: {a:?} / {b:?}");
                            }
                        }
                    }
                    unmount(cx, app, win);
                }
            }
        }
    }
}
