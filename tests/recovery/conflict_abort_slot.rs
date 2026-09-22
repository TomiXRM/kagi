//! #755: the window's single modal slot around the conflict Abort
//! confirmation — which focus its Escape resolves against, and which reload is
//! allowed to sweep it.
use crate::app_conflict::{click_control, content_fixture};
use crate::macos::{mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi_git::oplog::read_oplog_tail_for_repo;
use std::path::Path;

fn draw(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.run_until_parked();
}

/// Escape must reach the modal slot from the focus a user actually holds, not
/// only from the window root. Every other keyboard scenario focuses
/// `root_focus` before delivering the key (`press_key` in
/// `tests/recovery/operations.rs`), so the app's single Escape route — the
/// `CloseMainDiff` action — has never been exercised from the conflict
/// screen's own focus.
///
/// The reachable path: the Result pane mounts its code editor only in Edit
/// mode (`conflict_editor.rs`), so editing and switching back to Preview
/// leaves the window focused on an element that is no longer drawn. gpui then
/// dispatches from the tree root, where neither the routing wrapper's action
/// handler nor any key context exists, and every window action — Escape
/// included — silently does nothing. Opening the confirmation takes focus back
/// to the root, which is what this scenario holds.
pub fn scenario_conflict_abort_escape_focus(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();

    // Open the conflicting file and switch the Result pane to Edit, the only
    // mode that mounts the code editor. The parent's own render sync builds
    // the real `InputState`; nothing here is a test double.
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    draw(cx, window);
    let result_input = cx
        .read(|cx| {
            conflict
                .read(cx)
                .editor_inputs
                .as_ref()
                .map(|inputs| inputs.result.clone())
        })
        .expect("the Result pane's code editor is mounted");

    // The user clicks into the Result editor: its focus is live, so window
    // actions resolve against it.
    cx.update_window(window, |_, window, cx| {
        let handle = result_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            window.is_action_available(&kagi::ui::CloseMainDiff, cx)
        })
        .unwrap(),
        "precondition: a mounted, focused Result editor still reaches the \
         window's Escape action"
    );

    // …and switches back to Preview, which unmounts the editor under the
    // focus: the reachable dangling-focus path diagnosed in #755. (Which focus
    // #711's Esc was delivered into was never observed.)
    conflict.update(cx, |view, cx| {
        view.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    draw(cx, window);
    let text_before = cx.read(|cx| result_input.read(cx).value().to_string());

    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "precondition: the dashboard Abort opens the confirmation"
    );

    // Deliver Escape where the app left focus. The test never re-focuses.
    draw(cx, window);
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "Escape must close the Abort confirmation after the Result editor's \
         focus stopped being drawn"
    );
    assert_eq!(
        cx.read(|cx| result_input.read(cx).value().to_string()),
        text_before,
        "cancelling must not touch the resolution the editor is holding"
    );
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "and it mutates nothing"
    );
    assert!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .all(|entry| entry.op != "merge-abort"),
        "no abort may be recorded by a cancelled confirmation"
    );

    drop(result_input);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_abort_escape_focus");
}

/// Only an **accepted** reload may sweep the confirmation. `apply_reload_data`
/// hands its read to `accept_tab_view` first and returns when that read has
/// been superseded — by a newer read or by an admitted mutation — so the
/// confirmation the user is looking at survives a reload whose observation is
/// already out of date, and is closed by the next one that is not.
///
/// This scenario covers superseded same-owner reads; foreign-owner completion
/// is separately gated by the unchanged `active_session` guard in
/// `apply_reload_data`.
pub fn scenario_conflict_abort_superseded_reload(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();

    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "precondition: the dashboard Abort opens the confirmation"
    );

    // Start a reload and supersede its read before the dispatcher delivers it,
    // exactly as a later read or an admitted mutation does.
    app.update(cx, |app, cx| {
        app.reload(cx);
        let session = app.active_session().expect("owner");
        let _superseding = app.reads.begin(session);
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "a superseded reload must not close the confirmation on screen"
    );

    // …and the next accepted reload does close it, so the assertion above is
    // about the read being refused, not about the sweep being absent.
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "an accepted reload must sweep the confirmation"
    );

    assert!(repo.join(".git/MERGE_HEAD").exists());
    assert!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .all(|entry| entry.op != "merge-abort"),
        "neither reload may run the operation the confirmation was showing"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_abort_superseded_reload");
}
