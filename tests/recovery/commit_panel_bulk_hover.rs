//! #1496: semantic bulk captions retain a neutral hover, never an accent fill.
use super::*;

fn pointer(cx: &mut VisualTestAppContext, window: AnyWindowHandle, position: gpui::Point<Pixels>) {
    cx.simulate_event(
        window,
        gpui::MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: gpui::Modifiers::none(),
        },
    );
    paint(cx, window);
}

fn pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> [u8; 4] {
    let rect = bounds(window, id);
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native bulk button paint");
        let scale = window.scale_factor();
        // Interior padding, away from glyphs, rounded corners and focus border.
        image
            .get_pixel(
                ((f32::from(rect.left()) + 4.) * scale) as u32,
                ((f32::from(rect.top()) + f32::from(rect.size.height) / 2.) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

fn check(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    ids: &[&str],
    disabled: bool,
) {
    let before = crate::macos::repo_fingerprint(repo);
    for &id in ids {
        assert_eq!(labels::accessible_disabled(id), Some(disabled));
        pointer(cx, window, gpui::point(px(1.), px(1.)));
        let rest = pixel(cx, window, id);
        pointer(cx, window, bounds(window, id).center());
        assert_eq!(
            pixel(cx, window, id),
            rest,
            "{id}: semantic caption requires neutral hover"
        );
        if disabled {
            focus(cx, window, id);
            for key in ["enter", "space"] {
                press(cx, window, key);
            }
            assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
        }
    }
    assert_eq!(before, crate::macos::repo_fingerprint(repo));
}

pub fn scenario_commit_panel_bulk_hover(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme", "accent_source", "accent"]);
    for case in matrix().filter(|case| case.locale == "en") {
        for accent in [theme::SystemAccent::Blue, theme::SystemAccent::Yellow] {
            let _accent = theme::AccentInjection::new(accent, None);
            let fixture = build_fixture();
            std::fs::write(fixture.path().join("staged.txt"), "staged\n").unwrap();
            git(fixture.path(), &["add", "staged.txt"]);
            std::fs::write(fixture.path().join("README.md"), "unstaged tracked\n").unwrap();
            let (app, window) = focus_regressions::open_changes(cx, fixture.path(), case);
            app.update(cx, |_, cx| {
                theme::set_follows_system_accent(true);
                theme::sync_gpui_component_theme(cx);
                cx.notify();
            });
            paint(cx, window);
            check(
                cx,
                &app,
                window,
                fixture.path(),
                &["cp-stage-all", "cp-unstage-all", "cp-discard-all"],
                false,
            );
            unmount(cx, app, window);

            // A real merge conflict keeps disabled Stage/Discard present in both layouts.
            let fixture = conflict_fixture();
            let (app, window) = focus_regressions::open_changes(cx, fixture.path(), case);
            check(
                cx,
                &app,
                window,
                fixture.path(),
                &["cp-stage-all", "cp-discard-all"],
                true,
            );
            // Empty compact columns retain disabled Unstage; wide hides it by design.
            if case.dimensions.0 < 1000. {
                check(cx, &app, window, fixture.path(), &["cp-unstage-all"], true);
            }
            unmount(cx, app, window);
        }
    }
}
