//! Issue #935 — the in-app View menu (the Linux/FreeBSD menu bar) lists every
//! theme inline, so on a short window or at a high zoom it ran past the bottom
//! edge with no way to reach the last themes or the language rows.
//!
//! The dropdown renderer compiles on every target; only the Linux titlebar
//! opens it in the product. Here the View section is opened directly and the
//! laid-out bounds are read through the `e2e` canvas probes: the panel must end
//! inside the window, and a wheel over it must bring the last row into view.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, open_offscreen, repo_fingerprint, unmount};

/// GPUI snaps layout edges to device pixels.
const EPS: f32 = 1.;
/// Short enough that the View rows (commands, every theme, two languages)
/// cannot all fit, even at 1.0x.
const WINDOW: (f32, f32) = (1200., 480.);
const PANEL: &str = "platform-menu-panel";
const FIRST: &str = "platform-menu-cmd-view.zoomIn";
const LAST: &str = "platform-menu-cmd-lang.japanese";

fn mount_short(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let out = captured.clone();
    let win = open_offscreen(cx, size(px(WINDOW.0), px(WINDOW.1)), move |window, cx| {
        e2e::mount_root(state, window, cx, &out)
    });
    let app = captured.borrow().clone().expect("captured KagiApp");
    cx.run_until_parked();
    (app, win.into())
}

/// One real frame; the bounds each name had in it (`None`: not drawn).
fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    names: &[&str],
) -> Vec<Option<Bounds<Pixels>>> {
    let id = win.window_id();
    for name in names {
        e2e::clear_control_bounds(id, name);
    }
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the menu frame");
    names
        .iter()
        .map(|name| e2e::control_bounds(id, name))
        .collect()
}

fn inside(parent: Bounds<Pixels>, child: Bounds<Pixels>) -> bool {
    child.top() + px(EPS) >= parent.top() && child.bottom() <= parent.bottom() + px(EPS)
}

fn scroll(cx: &mut VisualTestAppContext, win: AnyWindowHandle, at: gpui::Point<Pixels>, dy: f32) {
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}

/// Open View at `zoom`, check the panel fits, scroll to the end, check the
/// last row is reachable, and close the menu again.
fn check_at(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, zoom: f32) {
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        assert!(
            app.open_platform_menu_for_e2e("View", cx),
            "the in-app menu bar has a View section"
        );
    });
    cx.run_until_parked();

    let frame = measure(cx, win, &[PANEL, FIRST, LAST]);
    let panel = frame[0].unwrap_or_else(|| panic!("{zoom}x: the View panel was not drawn"));
    let first = frame[1].expect("the first View row is drawn");
    let last = frame[2].expect("the last View row (Japanese) is laid out");
    assert!(
        panel.bottom() <= px(WINDOW.1) + px(EPS),
        "{zoom}x: the View panel {panel:?} runs past the {}px window",
        WINDOW.1
    );
    assert!(
        inside(panel, first),
        "{zoom}x: the first row starts in view"
    );
    assert!(
        !inside(panel, last),
        "{zoom}x: precondition — the rows must overflow this window for the \
         scenario to prove anything ({last:?} in {panel:?})"
    );

    // The panel probe sits inside the scrolling panel, so it moves with the
    // rows; `panel` from the unscrolled frame above is the visible box.
    scroll(cx, win, panel.center(), -10_000.);
    let frame = measure(cx, win, &[LAST]);
    let last = frame[0].expect("the last row is still laid out");
    assert!(
        inside(panel, last),
        "{zoom}x: a wheel over the panel brings the last row {last:?} into {panel:?}"
    );

    app.update(cx, |app, cx| {
        app.platform_menu_open = None;
        cx.notify();
    });
    cx.run_until_parked();
}

pub fn scenario_platform_menu_scroll(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["ui_zoom"]);
    let zoom_before = theme::zoom();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount_short(cx, &repo);

    check_at(cx, &app, win, 1.0);
    check_at(cx, &app, win, 1.5);

    theme::set_zoom(zoom_before);
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "opening a menu is not a write"
    );
    unmount(cx, app, win);
    eprintln!(
        "[gui-e2e] PASS platform_menu_scroll: the View menu fits a 480px window at 1.0x and 1.5x and scrolls to its last row"
    );
}

/// #976 review P1: the Linux / FreeBSD in-app menu dropdown is drawn after
/// the modal layer and opens from the titlebar even over a confirmation
/// modal. It is then the front layer: Enter must not confirm the modal behind
/// it (a Git write), and Escape closes the dropdown first.
pub fn scenario_platform_menu_over_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "dirty for the stash modal\n").unwrap();
    let (app, win) = mount_short(cx, &repo);
    let before = repo_fingerprint(&repo);
    let key = |cx: &mut VisualTestAppContext, keys: &str| {
        cx.update_window(win, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        cx.simulate_keystrokes(win, keys);
        cx.run_until_parked();
    };

    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    cx.run_until_parked();
    cx.update_window(win, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        window.focus(&root, cx);
    })
    .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).stash_push_modal().is_some()),
        "precondition: the Stash confirmation is open"
    );
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx), "View section");
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)),
        "platform-menu-over-modal-front: the dropdown drawn over the modal must be the front layer"
    );

    key(cx, "enter");
    assert!(
        cx.read(|cx| app.read(cx).stash_push_modal().is_some()),
        "platform-menu-over-modal-enter: Enter confirmed the modal behind the dropdown"
    );
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "platform-menu-over-modal-enter: Enter behind the dropdown wrote the repository"
    );

    key(cx, "escape");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.platform_menu_open.is_none(),
            "Escape closes the dropdown first"
        );
        assert!(
            app.stash_push_modal().is_some(),
            "and leaves the modal open"
        );
    });
    key(cx, "escape");
    assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_none()));
    assert_eq!(repo_fingerprint(&repo), before, "nothing was written");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_over_modal");
}
