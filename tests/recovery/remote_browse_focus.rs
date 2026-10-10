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

fn paint_browser(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    draw(cx, window);
    draw(cx, window);
}

fn press(cx: &mut VisualTestAppContext, window: AnyWindowHandle, keys: &str) {
    // GPUI activates a focused row on key-up, not key-down alone.
    draw(cx, window);
    let keystroke = gpui::Keystroke::parse(keys).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    paint_browser(cx, window);
}

fn focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Option<usize> {
    cx.update_window(window, |_, window, cx| {
        app.read(cx)
            .remote_browse()
            .and_then(|modal| e2e_transport::focused_row(modal, window))
    })
    .unwrap()
}

fn assert_revealed(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, row: usize) {
    cx.read(|cx| {
        let modal = app.read(cx).remote_browse().unwrap();
        let bounds = e2e_transport::row_bounds(modal, row).expect("focused row must be mounted");
        let viewport = e2e_transport::viewport(modal);
        assert!(
            bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
            "keyboard selection must reveal the whole row: {bounds:?} in {viewport:?}"
        );
    });
}

fn connect_listing(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cwd: &str,
    listing: &str,
) {
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    draw(cx, window);
    paste_host(cx, app, window);
    let outcome = e2e_transport::connected(cwd, listing, "false\n", "");
    e2e_transport::queue_remote_connect(cx.background_executor.spawn(async move { outcome }));
    // Start through the form's real Connect button. Directory keyboard
    // navigation starts after the accepted read mounts the RowList.
    let connect = kagi::ui::e2e::control_bounds(window.window_id(), "remote-connect-go").unwrap();
    cx.simulate_click(window, connect.center(), gpui::Modifiers::none());
    paint_browser(cx, window);
    assert_eq!(
        focused(cx, app, window),
        Some(0),
        "accepted read focuses its first row"
    );
}

fn navigate_listing(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    path: &str,
    listing: &str,
    activation: &str,
) {
    let outcome = e2e_transport::connected(path, listing, "false\n", "");
    e2e_transport::queue_remote_navigate(
        path,
        cx.background_executor.spawn(async move { outcome }),
    );
    press(cx, window, activation);
}

/// #1071: real input → accepted focus → roving virtual rows. No synthetic
/// focus calls, source pins, or invented SSH timing are used.
pub fn scenario_remote_browse_keyboard_rows(cx: &mut VisualTestAppContext) {
    use kagi::ui::{
        i18n::{self, Lang},
        list_a11y,
    };
    let _settings = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    let listing = (0..300)
        .map(|ix| format!("dir-{ix:03}/\n"))
        .chain(std::iter::once("notes.txt\n".to_string()))
        .collect::<String>();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        connect_listing(cx, &app, window, "/home/dev", &listing);
        list_a11y::clear_recorded_lists();
        draw(cx, window);
        let recorded = list_a11y::recorded_list("remote-dir-list").unwrap();
        assert_eq!(recorded.role, Some(gpui::Role::ListBox));
        assert_eq!(recorded.size, 302);
        assert!(
            recorded.rows.len() < 302,
            "offscreen rows must not be rendered"
        );
        assert!(recorded.rows[&0].1, "the accepted first row is selected");
        assert!(recorded.label.contains(match lang {
            Lang::En => "directory",
            Lang::Ja => "ディレクトリ",
        }));
        assert!(recorded.rows[&0].0.contains(match lang {
            Lang::En => "Parent",
            Lang::Ja => "親",
        }));

        // Repeated Down must reveal selection beyond the initial viewport,
        // and Enter must use that exact row rather than the current directory.
        kagi::ui::e2e::clear_control_bounds(window.window_id(), "remote-dir-row-40");
        for _ in 0..40 {
            press(cx, window, "down");
        }
        assert_eq!(focused(cx, &app, window), Some(40));
        assert_revealed(cx, &app, 40);
        let control = kagi::ui::e2e::control_bounds(window.window_id(), "remote-dir-row-40")
            .expect("the selected row control must be drawn");
        let viewport = cx.read(|cx| e2e_transport::viewport(app.read(cx).remote_browse().unwrap()));
        assert!(
            control.top() >= viewport.top() && control.bottom() <= viewport.bottom(),
            "selected control {control:?} must be wholly inside {viewport:?}"
        );
        navigate_listing(cx, window, "/home/dev/dir-039", "nested/\n", "enter");
        assert_eq!(
            cx.read(|cx| app.read(cx).remote_browse().unwrap().cwd.clone()),
            "/home/dev/dir-039"
        );
        navigate_listing(cx, window, "/home/dev", &listing, "enter");

        press(cx, window, "down");
        assert_eq!(focused(cx, &app, window), Some(1));
        press(cx, window, "pagedown");
        let page = focused(cx, &app, window).unwrap();
        assert!(
            page > 1 && page < 300,
            "PageDown advances by the visible page, not to the end"
        );
        assert_revealed(cx, &app, page);
        press(cx, window, "pageup");
        assert!(focused(cx, &app, window).unwrap() < page);
        press(cx, window, "home");
        press(cx, window, "up");
        press(cx, window, "pageup");
        assert_eq!(focused(cx, &app, window), Some(0), "first boundary clamps");
        press(cx, window, "end");
        assert_eq!(focused(cx, &app, window), Some(301));
        assert_revealed(cx, &app, 301);
        press(cx, window, "down");
        press(cx, window, "pagedown");
        assert_eq!(focused(cx, &app, window), Some(301), "last boundary clamps");
        press(cx, window, "enter");
        press(cx, window, "space");
        cx.read(|cx| {
            let state = app.read(cx);
            let modal = state
                .remote_browse()
                .expect("files must not open a repository");
            assert_eq!(modal.cwd, "/home/dev");
            assert!(!modal.busy, "a file starts no directory read");
            assert!(state.remote_view.is_none());
        });
        press(cx, window, "up");
        assert_eq!(focused(cx, &app, window), Some(300));
        assert_revealed(cx, &app, 300);
        let file = cx
            .read(|cx| e2e_transport::row_bounds(app.read(cx).remote_browse().unwrap(), 301))
            .unwrap();
        cx.simulate_click(window, file.center(), gpui::Modifiers::none());
        paint_browser(cx, window);
        assert_eq!(
            focused(cx, &app, window),
            Some(301),
            "pointer selection uses the same row owner"
        );
        assert!(!cx.read(|cx| app.read(cx).remote_browse().unwrap().busy));
        press(cx, window, "up");
        navigate_listing(cx, window, "/home/dev/dir-299", "nested/\n", "enter");
        assert_eq!(focused(cx, &app, window), Some(0));
        press(cx, window, "down");
        kagi::ui::e2e::clear_control_bounds(window.window_id(), "remote-dir-empty");
        kagi::ui::e2e::clear_control_bounds(window.window_id(), "remote-dir-row-0");
        navigate_listing(cx, window, "/home/dev/dir-299/nested", "", "space");
        assert_eq!(focused(cx, &app, window), Some(0));
        let empty = kagi::ui::e2e::control_bounds(window.window_id(), "remote-dir-empty")
            .expect("an empty nested directory draws its empty notice");
        let parent = kagi::ui::e2e::control_bounds(window.window_id(), "remote-dir-row-0")
            .expect("an empty nested directory keeps its parent row");
        assert!(
            empty.top() >= parent.bottom(),
            "empty notice {empty:?} must be below parent {parent:?}"
        );
        assert_eq!(
            kagi::ui::dialog_a11y::recorded_note("remote-dir-empty"),
            Some((
                gpui::Role::Note,
                kagi::ui::i18n::Msg::RemoteDirectoryEmpty.t().to_string()
            )),
            "the drawn empty notice carries the current EN/JA text"
        );
        navigate_listing(cx, window, "/home/dev/dir-299", "nested/\n", "enter");
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).remote_browse().unwrap().cwd,
                "/home/dev/dir-299"
            )
        });
        navigate_listing(cx, window, "/home/dev", &listing, "space");
        assert_eq!(focused(cx, &app, window), Some(0));
        // One Tab leaves all 302 rows, and Shift+Tab returns to the list.
        press(cx, window, "tab");
        assert_eq!(focused(cx, &app, window), None);
        press(cx, window, "shift-tab");
        assert_eq!(focused(cx, &app, window), Some(0));
        press(cx, window, "escape");
        assert!(cx.read(|cx| app.read(cx).remote_browse().is_none()));
        assert!(
            cx.update_window(window, |_, window, cx| app
                .read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window)))
                .unwrap(),
            "Escape from a directory row must return focus to the window"
        );
    }
    i18n::set_lang(language);
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_browse_keyboard_rows");
}

/// #1071: pending/failed navigation keeps its accepted owner and selection;
/// Escape and reopening invalidate late read acceptance and its focus request.
pub fn scenario_remote_browse_keyboard_read_ownership(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    connect_listing(cx, &app, window, "/home/dev", "src/\nnotes.txt\n");
    press(cx, window, "down");
    let timer = cx.background_executor.clone();
    e2e_transport::queue_remote_navigate(
        "/home/dev/src",
        cx.background_executor.spawn(async move {
            timer.timer(Duration::from_secs(1)).await;
            e2e_transport::refused(REFUSAL)
        }),
    );
    press(cx, window, "enter");
    press(cx, window, "enter");
    press(cx, window, "space");
    cx.read(|cx| {
        let modal = app.read(cx).remote_browse().unwrap();
        assert!(modal.busy);
        assert_eq!(modal.cwd, "/home/dev");
        assert_eq!(modal.entries.len(), 2);
    });
    assert_eq!(
        focused(cx, &app, window),
        Some(1),
        "loading retains the accepted selection"
    );
    cx.advance_clock(Duration::from_secs(1));
    paint_browser(cx, window);
    cx.read(|cx| {
        let modal = app.read(cx).remote_browse().unwrap();
        assert!(!modal.busy);
        assert!(modal.error.is_some());
        assert_eq!(modal.cwd, "/home/dev");
    });
    assert_eq!(
        focused(cx, &app, window),
        Some(1),
        "failed read must not replace rows or focus"
    );

    let timer = cx.background_executor.clone();
    e2e_transport::queue_remote_navigate(
        "/home/dev/src",
        cx.background_executor.spawn(async move {
            timer.timer(Duration::from_secs(1)).await;
            e2e_transport::connected("/home/dev/src", "stale/\n", "false\n", "")
        }),
    );
    press(cx, window, "space");
    press(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).remote_browse().is_none()));
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    draw(cx, window);
    let host = cx
        .read(|cx| app.read(cx).remote_browse().unwrap().host_state.clone())
        .unwrap();
    cx.advance_clock(Duration::from_secs(1));
    paint_browser(cx, window);
    cx.read(|cx| {
        let modal = app.read(cx).remote_browse().unwrap();
        assert!(modal.stage == RemoteBrowseStage::Connect);
        assert!(
            modal.entries.is_empty(),
            "old rows never enter the reopened owner"
        );
    });
    assert!(cx
        .update_window(window, |_, window, cx| host
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
        .unwrap());
    // The reopened input remains usable after the late completion.
    paste_host(cx, &app, window);
    e2e_transport::queue_remote_connect(
        cx.background_executor
            .spawn(async move { e2e_transport::connected("/", "", "false\n", "") }),
    );
    let connect = kagi::ui::e2e::control_bounds(window.window_id(), "remote-connect-go").unwrap();
    cx.simulate_click(window, connect.center(), gpui::Modifiers::none());
    paint_browser(cx, window);
    assert_eq!(
        focused(cx, &app, window),
        None,
        "an empty root has no phantom parent row"
    );
    cx.read(|cx| {
        let modal = app.read(cx).remote_browse().unwrap();
        assert_eq!(modal.cwd, "/");
        assert!(modal.entries.is_empty());
        assert!(!modal.current_is_repo);
    });
    press(cx, window, "enter");
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_some()),
        "the card's Enter fallback must not open a non-repository directory"
    );
    assert!(!cx.read(|cx| app.read(cx).remote_browse().unwrap().busy));
    press(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).remote_view.is_none()));
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_browse_keyboard_read_ownership");
}
