//! Restarting partial source decisions keeps remaining adoption controls usable.
use super::*;
use kagi::ui::conflict_editor_a11y as ax;

pub fn scenario_conflict_partial_resume(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    for action in ["use", "both"] {
        let fixture = fixture();
        let before = repo_fingerprint(fixture.path());
        let index = std::fs::read(fixture.path().join(".git/index")).unwrap();
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            app.set_lang(Lang::Ja, cx);
            app.detect_conflict_mode(cx);
        });
        cx.run_until_parked();
        click_control(cx, window, "conflict-hunk-0-current-use");
        let expected = cx.read(|cx| {
            let view = app.read(cx).ui().conflict.as_ref().unwrap().read(cx);
            let model = view
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .hunk_model(Path::new("a.txt"))
                .unwrap();
            assert_eq!(model.resolved_hunk_count(), 1);
            model.clone()
        });
        unmount(cx, app, window);
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        click_control(cx, window, "conflict-row-0");
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            let view = app.read(cx).ui().conflict.as_ref().unwrap().read(cx);
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .hunk_model(Path::new("a.txt")),
                Some(&expected),
                "source choices survive remount"
            );
        });
        for (side, label) in [("current", "採用"), ("incoming", "不採用")] {
            assert_eq!(
                ax::control(&format!("conflict-hunk-0-{side}-state"))
                    .unwrap()
                    .1,
                label
            );
        }
        click_control(cx, window, "conflict-next-hunk");
        for side in ["current", "incoming"] {
            for command in ["use", "both"] {
                assert!(e2e::control_bounds(
                    window.window_id(),
                    &format!("conflict-hunk-1-{side}-{command}")
                )
                .is_some());
            }
        }
        click_control(cx, window, &format!("conflict-hunk-1-current-{action}"));
        cx.run_until_parked();
        cx.read(|cx| {
            let view = app.read(cx).ui().conflict.as_ref().unwrap().read(cx);
            let buffer = &view.mode.as_ref().unwrap().buffer;
            assert_eq!(
                buffer
                    .hunk_model(Path::new("a.txt"))
                    .unwrap()
                    .resolved_hunk_count(),
                2,
                "remaining {action} control is enabled after restart"
            );
            assert!(buffer
                .resolved_text(Path::new("a.txt"))
                .unwrap()
                .contains("a.txt current first"));
        });
        assert_eq!(before, repo_fingerprint(fixture.path()));
        assert_eq!(
            index,
            std::fs::read(fixture.path().join(".git/index")).unwrap()
        );
        unmount(cx, app, window);
    }
    i18n::set_lang(original);
}
