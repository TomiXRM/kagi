//! #497: actual Result-input edits must publish one verdict, not one per repaint.
use crate::app_conflict::content_fixture;
use crate::macos::{mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::i18n::Msg;
use kagi_git::resolution::SelectionSide;
use std::path::Path;

fn draw(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.run_until_parked();
}

pub fn scenario_conflict_continue_cache(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before_repo = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    let assert_verdict = |cx: &mut VisualTestAppContext, expected: Option<Msg>| {
        cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().unwrap();
            assert_eq!(mode.continue_blocker(), expected);
            assert_eq!(mode.can_continue(), expected.is_none());
        });
    };
    assert_verdict(cx, Some(Msg::ConflictBlockerUnresolved));
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    app.update(cx, |_, cx| cx.notify());
    draw(cx, window);
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });

    // Missing final LF must not become a phantom edit on every render. Empty
    // text and one blank line remain different resolutions. Then add/remove a
    // marker through the real InputState and the production parent sync pass.
    for (text, result, blocker) in [
        ("resolved", "resolved\n", None),
        ("", "", None),
        ("\n", "\n", None),
        (
            "<<<<<<< current\nbad\n",
            "<<<<<<< current\nbad\n",
            Some(Msg::ConflictBlockerMarker),
        ),
        ("resolved\n", "resolved\n", None),
    ] {
        cx.update_window(window, |_, window, cx| {
            input.update(cx, |input, cx| input.set_value(text, window, cx));
            app.update(cx, |_, cx| cx.notify());
        })
        .unwrap();
        draw(cx, window);
        assert_verdict(cx, blocker);
        cx.read(|cx| {
            let view = conflict.read(cx);
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new("file.txt"))
                    .as_deref(),
                Some(result)
            );
        });
        // An unchanged parent repaint must not create another undo entry.
        app.update(cx, |_, cx| cx.notify());
        draw(cx, window);
        assert_verdict(cx, blocker);
    }

    // Preview does not feed stale InputState text back over a restored Result.
    conflict.update(cx, |view, cx| {
        view.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    app.update(cx, |_, cx| cx.notify());
    draw(cx, window);
    conflict.update(cx, |view, cx| {
        assert!(view
            .mode
            .as_mut()
            .unwrap()
            .buffer
            .undo(Path::new("file.txt")));
        cx.notify();
    });
    draw(cx, window);
    assert_verdict(cx, Some(Msg::ConflictBlockerMarker));
    conflict.update(cx, |view, cx| {
        assert!(view
            .mode
            .as_mut()
            .unwrap()
            .buffer
            .redo(Path::new("file.txt")));
        cx.notify();
    });
    draw(cx, window);
    assert_verdict(cx, None);
    conflict.update(cx, |view, cx| {
        assert!(view
            .mode
            .as_mut()
            .unwrap()
            .buffer
            .undo(Path::new("file.txt")));
        view.conflict_editor_set_file_side(Path::new("file.txt"), SelectionSide::Current, true);
        cx.notify();
    });
    app.update(cx, |_, cx| cx.notify());
    draw(cx, window);
    assert_verdict(cx, None);
    assert_eq!(
        repo_fingerprint(&repo),
        before_repo,
        "draft edits must not write the repository"
    );
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_continue_cache");
}
