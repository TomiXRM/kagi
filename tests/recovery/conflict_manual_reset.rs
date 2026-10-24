//! Per-hunk controls must never discard another hunk's manual Result text.
use super::*;

pub fn scenario_conflict_manual_reset_guard(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    click_control(cx, window, "conflict-hunk-0-current-use");
    click_control(cx, window, "conflict-result-editor");
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    let manual = cx.read(|cx| {
        input
            .read(cx)
            .value()
            .replace("a.txt incoming last", "MANUAL HUNK TWO")
    });
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_input(window, &manual);
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(manual.contains("MANUAL HUNK TWO"));
    for id in [
        "conflict-reset-hunk",
        "conflict-hunk-0-current-change",
        "conflict-hunk-0-incoming-change",
    ] {
        click_control(cx, window, id);
        assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), manual);
    }
    // Direct handler calls recheck the same policy, not only button appearance.
    conflict.update(cx, |view, cx| {
        view.conflict_editor_reset_hunk(Path::new("a.txt"), 0);
        assert_eq!(
            view.mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("a.txt"))
                .as_deref(),
            Some(manual.as_str())
        );
        cx.notify();
    });
    click_control(cx, window, "conflict-reset-all");
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), manual);
    cx.read(|cx| {
        assert_eq!(
            conflict.read(cx).reset_all_target.as_deref(),
            Some(Path::new("a.txt"))
        )
    });
    click_control(cx, window, "conflict-reset-all");
    cx.run_until_parked();
    assert!(!cx.read(|cx| input.read(cx).value().contains("MANUAL HUNK TWO")));
    assert_eq!(before, repo_fingerprint(fixture.path()));
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
}
