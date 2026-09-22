//! #755: Escape must close Remote Browse after its focused Connect input is
//! replaced by the Browse listing. No test-side refocusing masks the transition.
//! Only the SSH read result is supplied; validation, generation checks and
//! asynchronous completion remain production code. Rejected and superseded
//! results must preserve the connection form's focus.

use std::time::Duration;

use gpui::{AnyWindowHandle, ClipboardItem, Entity, Focusable, VisualTestAppContext};
use kagi::ui::remote_browse::{e2e_transport, RemoteBrowseStage};
use kagi::ui::KagiApp;

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};

/// A host spec the form accepts; nothing ever dials it.
const HOST: &str = "dev@build-host";
const REFUSAL: &str = "ssh: connect to host build-host port 22: Connection refused";

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

/// Type the host into whichever field the modal focused, the way a user does:
/// a real paste into the real `InputState`, which lands only while the
/// connection form holds the window's focus. The following draw is the form's
/// own render sync copying the field into `host_input`.
fn paste_host(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(HOST.to_string()));
    });
    cx.simulate_keystrokes(window, "cmd-v");
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .remote_browse()
            .map(|modal| modal.host_input.clone())),
        Some(HOST.to_string()),
        "the focused host field must receive what the user typed"
    );
}

pub fn scenario_remote_browse_escape_focus(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);

    // The product opens its own connection form and focuses its own host field.
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    draw(cx, window);
    let connect_host = cx
        .read(|cx| {
            app.read(cx)
                .remote_browse()
                .and_then(|modal| modal.host_state.clone())
        })
        .expect("the connection form mounts its host input");
    assert!(
        cx.update_window(window, |_, window, cx| connect_host
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
            .unwrap(),
        "precondition: the connection form holds the window's focus"
    );
    assert!(
        cx.update_window(window, |_, window, cx| window
            .is_action_available(&kagi::ui::CloseMainDiff, cx))
            .unwrap(),
        "precondition: Escape reaches the modal slot from the focused host field"
    );

    paste_host(cx, &app, window);

    // ── Refused connection: the form stays, and keeps the typed field focused.
    e2e_transport::queue_remote_connect(
        cx.background_executor
            .spawn(async move { e2e_transport::refused(REFUSAL) }),
    );
    app.update(cx, |app, cx| app.start_remote_connect(cx));
    cx.run_until_parked();
    draw(cx, window);
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .remote_browse()
            .expect("a refused connect keeps the modal open");
        assert!(
            modal.stage == RemoteBrowseStage::Connect,
            "a refused connect must not browse anything"
        );
    });
    assert!(
        cx.update_window(window, |_, window, cx| connect_host
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
            .unwrap(),
        "a refused completion must leave the typed host field focused"
    );

    // ── Superseded connection: a reopened form is a new generation, so the
    // in-flight read may neither transition it nor take its focus.
    let stale_timer = cx.background_executor.clone();
    e2e_transport::queue_remote_connect(cx.background_executor.spawn(async move {
        stale_timer.timer(Duration::from_secs(1)).await;
        e2e_transport::connected(
            "/home/stale",
            "stale-repo/\n",
            "true\n/home/stale\n",
            "0000000\u{1f}HEAD -> stale\u{1f}stale head\n",
        )
    }));
    app.update(cx, |app, cx| app.start_remote_connect(cx));
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    draw(cx, window);
    let reopened_host = cx
        .read(|cx| {
            app.read(cx)
                .remote_browse()
                .and_then(|modal| modal.host_state.clone())
        })
        .expect("the reopened form mounts a fresh host input");
    assert!(
        connect_host.entity_id() != reopened_host.entity_id(),
        "precondition: reopening builds a new form rather than reusing the old one"
    );
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    draw(cx, window);
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .remote_browse()
            .expect("the reopened modal stays open");
        assert!(
            modal.stage == RemoteBrowseStage::Connect,
            "a superseded connect must not browse the reopened form"
        );
    });
    assert!(
        cx.update_window(window, |_, window, cx| reopened_host
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
            .unwrap(),
        "a superseded completion must not take focus from the reopened form"
    );

    // ── Accepted connection: the form becomes the directory browser.
    paste_host(cx, &app, window);
    e2e_transport::queue_remote_connect(cx.background_executor.spawn(async move {
        e2e_transport::connected(
            "/home/dev",
            "src/\nnotes.txt\n",
            "true\n/home/dev\n",
            "abc1234\u{1f}HEAD -> main\u{1f}initial commit\n",
        )
    }));
    app.update(cx, |app, cx| app.start_remote_connect(cx));
    cx.run_until_parked();
    draw(cx, window);
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .remote_browse()
            .expect("an accepted connect keeps the slot");
        assert!(
            modal.stage == RemoteBrowseStage::Browse,
            "an accepted connect must show the directory browser"
        );
    });

    // Deliver Escape where the app left focus. The test never re-focuses.
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_none()),
        "Escape must close Remote Browse after the connection form's focused \
         input stopped being drawn"
    );
    assert!(
        cx.read(|cx| app.read(cx).remote_view.is_none()),
        "and a cancelled browse must open no remote repository"
    );
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "the browsed host is read-only: the open repository must be untouched"
    );

    drop(connect_host);
    drop(reopened_host);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_browse_escape_focus");
}
