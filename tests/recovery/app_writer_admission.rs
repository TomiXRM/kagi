//! Deterministic editor → host admission wiring; real remove contention is G/M.
use crate::macos::{build_fixture, mount};
use gpui::{Focusable, VisualTestAppContext};
use kagi::app::LegacyBusy;
use kagi::ui::{i18n::Msg, FooterStatus};
use std::time::{Duration, Instant};

pub fn scenario_editor_save_admission(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx.read(|cx| app.read(cx).editor_workspace.clone()).unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    // Same Sessions API as the bridge; no child process or pending remove
    // future for the real-platform dispatcher to wait on.
    let guard = app.update(cx, |app, _| {
        app.app_sessions
            .write_lease(&repo, LegacyBusy(false))
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));
    let edited = cx.read(|cx| {
        editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()
    });
    assert_ne!(edited.as_bytes(), before.as_slice());

    // Exercise save_editor_file → SaveRequested → reserve_write, not a direct
    // invocation of the pane executor or a manually raised Busy notification.
    app.update(cx, |app, cx| app.save_editor_file(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "Busy must retain the buffer"
    );
    assert!(cx.read(|cx| matches!(&app.read(cx).status_footer,
        FooterStatus::Failed(message) if message.as_ref() == Msg::OpInProgress.t())));
    assert!(cx.read(|cx| app
        .read(cx)
        .toast_stack
        .as_ref()
        .unwrap()
        .read(cx)
        .toasts()
        .iter()
        .any(|toast| toast.message.as_ref() == Msg::OpInProgress.t())));
    assert_eq!(
        cx.read(|cx| editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        edited
    );

    guard.complete();
    assert!(cx.read(|cx| !app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| app.save_editor_file(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !editor.read(cx).dirty && !app.read(cx).app_sessions.has_leases()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "admitted editor save did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        std::fs::read(repo.join("README.md")).unwrap(),
        edited.as_bytes()
    );
    eprintln!("[gui-e2e] PASS editor admission → Busy preserves bytes/buffer → release → save writes edited bytes");
}
