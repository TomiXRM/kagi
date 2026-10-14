//! Graph is one keyboard stop between Sidebar and Inspector (#1532).
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{i18n::Lang, theme, KagiApp};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}
fn graph_focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        app.read(cx)
            .graph_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
    })
    .unwrap()
}
pub fn scenario_graph_tab_stop(cx: &mut VisualTestAppContext) {
    let _saved = crate::recovery_layout::GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let fixture = build_fixture();
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for slug in ["apple-light", "apple-dark"] {
                let (app, window) = mount(cx, fixture.path());
                cx.run_until_parked();
                theme::set_zoom(zoom);
                app.update(cx, |app, cx| {
                    app.set_lang(lang, cx);
                    app.set_theme(slug, cx);
                });
                crate::pr_detail_adaptive::resize(cx, window, (1000., 720.));
                crate::native_ax::activate(cx);
                draw(cx, window);
                let mut reached = false;
                let mut sidebar_before = false;
                for _ in 0..120 {
                    sidebar_before = cx
                        .update_window(window, |_, window, cx| {
                            let app = app.read(cx);
                            app.sidebar_row_focused_for_e2e(window).is_some()
                                || app.sidebar_header_focused_for_e2e(window).is_some()
                        })
                        .unwrap();
                    keys(cx, window, "tab");
                    draw(cx, window);
                    if graph_focused(cx, &app, window) {
                        reached = true;
                        break;
                    }
                }
                assert!(
                    reached,
                    "Tab did not reach Graph ({lang:?}, {zoom}, {slug})"
                );
                assert!(sidebar_before, "Graph follows Sidebar");
                let bounds =
                    kagi::ui::e2e::control_bounds(window.window_id(), "commit-list-viewport")
                        .unwrap();
                cx.update_window(window, |_, window, _| {
                    let image = window.render_to_image().expect("Graph native ring image");
                    let scale = window.scale_factor();
                    let pixel = image
                        .get_pixel(
                            (f32::from(bounds.left() + gpui::px(1.)) * scale) as u32,
                            (f32::from(bounds.center().y) * scale) as u32,
                        )
                        .0;
                    let accent = theme::theme().ui_accent();
                    for (got, expected) in pixel[..3].iter().zip([
                        (accent >> 16) as u8,
                        (accent >> 8) as u8,
                        accent as u8,
                    ]) {
                        assert!(
                            got.abs_diff(expected) <= 2,
                            "Graph ring {pixel:?}, expected {accent:06x}"
                        );
                    }
                })
                .unwrap();
                let before = cx.read(|cx| app.read(cx).ui().selected);
                keys(cx, window, "down");
                draw(cx, window);
                let after = cx.read(|cx| app.read(cx).ui().selected);
                assert_ne!(before, after, "Graph Down moves commit selection");
                assert!(graph_focused(cx, &app, window));
                keys(cx, window, "down");
                draw(cx, window);
                assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
                keys(cx, window, "up");
                draw(cx, window);
                assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
                for bottom_open in [true, false] {
                    app.update(cx, |app, cx| {
                        app.bottom_panel_open = bottom_open;
                        cx.notify();
                    });
                    draw(cx, window);
                    keys(cx, window, "tab");
                    draw(cx, window);
                    assert!(!graph_focused(cx, &app, window), "Graph has one stop");
                    let tree = crate::native_ax::tree(cx, window);
                    let focused = tree["gpui_focus"]
                        .as_str()
                        .expect("next control owns focus");
                    let next = if bottom_open {
                        "tab-terminal"
                    } else {
                        "inspector-hash-chip"
                    };
                    assert_eq!(
                        tree["nodes"][focused]["element_id"],
                        format!("{:?}", gpui::ElementId::from(next)),
                        "Tab preserves bottom controls before Inspector when open"
                    );
                    keys(cx, window, "shift-tab");
                    draw(cx, window);
                    assert!(
                        graph_focused(cx, &app, window),
                        "Shift-Tab returns to Graph"
                    );
                }
                unmount(cx, app, window);
            }
        }
    }
}
