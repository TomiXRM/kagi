//! #1098: the real four-action PR menu must fit, including Copy URL's hitbox.
//! No action is pressed: browser and host clipboard are deliberately untouched.
use std::cell::RefCell;
use std::rc::Rc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, open_offscreen, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};

const ROWS: [&str; 4] = [
    "pr-menu-peek",
    "pr-menu-jump",
    "pr-menu-open",
    "pr-menu-copy",
];

pub fn scenario_pr_menu_bounds(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for dimensions in [(1392., 883.), (940., 660.)] {
                for table in [false, true] {
                    let state = e2e::app_state(fixture.path()).expect("fixture app state");
                    let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
                    let output = captured.clone();
                    let window: AnyWindowHandle = open_offscreen(
                        cx,
                        size(px(dimensions.0), px(dimensions.1)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    )
                    .into();
                    let app = captured.borrow().clone().expect("captured KagiApp");
                    cx.run_until_parked();
                    let pr =
                        crate::evidence_support::pull_request(112, "Bottom-edge PR", "feature");
                    app.update(cx, |app, cx| {
                        if table {
                            app.show_pr_mode(cx);
                        }
                        app.ui_mut().expect("active session").github_prs = vec![pr.clone()];
                        cx.notify();
                    });
                    cx.run_until_parked();
                    let viewport = Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(dimensions.0), px(dimensions.1)),
                    );
                    // Bottom first reproduces the owned native failure before other corners.
                    for anchor in [
                        point(px(dimensions.0 - 2.), px(dimensions.1 - 2.)),
                        point(px(2.), px(dimensions.1 - 2.)),
                        point(px(2.), px(2.)),
                        point(px(dimensions.0 - 2.), px(2.)),
                    ] {
                        app.update(cx, |app, cx| {
                            app.ui_mut().expect("active session").pr_menu =
                                Some((pr.clone(), anchor));
                            cx.notify();
                        });
                        for row in ROWS {
                            // Clear paint samples as well, so this frame must produce them.
                            e2e::clear_control_bounds(window.window_id(), row);
                        }
                        e2e::clear_control_bounds(window.window_id(), "pr-menu-footer");
                        draw(cx, window, dimensions);
                        let label =
                            format!("{locale}/{zoom}/{dimensions:?}/table={table}/{anchor:?}");
                        let mut previous_bottom = px(0.);
                        let footer = e2e::control_paint(window.window_id(), "pr-menu-footer")
                            .expect("actual footer paint recorded");
                        for row in ROWS {
                            let bounds = e2e::control_bounds(window.window_id(), row)
                                .unwrap_or_else(|| panic!("{label}/{row}: not rendered"));
                            assert!(
                                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                                "{label}/{row}: empty row"
                            );
                            contained(viewport, bounds, &format!("{label}/{row}"));
                            let paint = e2e::control_paint(window.window_id(), row)
                                .unwrap_or_else(|| panic!("{label}/{row}: not painted"));
                            contained(
                                paint.mask,
                                paint.bounds,
                                &format!("{label}/{row}/paint-mask"),
                            );
                            let covered = paint.bounds.left() < footer.bounds.right()
                                && paint.bounds.right() > footer.bounds.left()
                                && paint.bounds.top() < footer.bounds.bottom()
                                && paint.bounds.bottom() > footer.bounds.top();
                            assert!(
                                !covered || paint.order > footer.order,
                                "{label}/{row}: later opaque footer overpaints row: row={paint:?} footer={footer:?}"
                            );
                            assert!(
                                bounds.top() >= previous_bottom,
                                "{label}/{row}: rows overlap or are out of order"
                            );
                            previous_bottom = bounds.bottom();
                        }
                        let copy = e2e::control_bounds(window.window_id(), "pr-menu-copy").unwrap();
                        cx.simulate_mouse_move(
                            window,
                            point(px(0.), px(0.)),
                            None,
                            Modifiers::none(),
                        );
                        draw(cx, window, dimensions);
                        cx.simulate_mouse_move(window, copy.center(), None, Modifiers::none());
                        draw(cx, window, dimensions);
                        assert!(
                            e2e::pr_menu_copy_hovered(),
                            "{label}: actual fourth-row hitbox does not receive hover"
                        );
                        // Dismiss through the real backdrop, without pressing any PR command.
                        cx.simulate_click(
                            window,
                            point(px(dimensions.0 / 2.), px(dimensions.1 / 2.)),
                            Modifiers::none(),
                        );
                        cx.run_until_parked();
                        assert!(
                            cx.read(|cx| app.read(cx).ui().pr_menu.is_none()),
                            "{label}: outside click did not dismiss"
                        );
                    }
                    unmount(cx, app, window);
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS pr_menu_bounds: four rendered PR rows and fourth-row hover fit all corners, sidebar/table EN/JA 100/167% normal/narrow");
}
