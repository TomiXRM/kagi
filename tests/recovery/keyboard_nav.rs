//! Keyboard paths of Kagi's tab lists (#944), Tier A.
//!
//! The test dispatcher cannot press Tab or prove focus-visible, so a cell is
//! focused through a hook and the keys are pressed on it; what each key does
//! is checked. Rows are covered in `home_work` (Enter / Space on a row).

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::workspace_mode::WorkspaceMode;
use kagi::ui::{e2e, KagiApp};

use crate::app_conflict::click_control;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

fn mode(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> WorkspaceMode {
    cx.read(|cx| app.read(cx).workspace_mode())
}

fn pane(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> HomePane {
    cx.read(|cx| app.read(cx).home_github.work.pane)
}

fn root_focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        app.read(cx)
            .root_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
    })
    .unwrap()
}

/// The workspace-mode cell holding the focus, if any.
fn nav_focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Option<usize> {
    cx.update_window(window, |_, window, cx| {
        app.read(cx).mode_nav_focused_for_e2e(window)
    })
    .unwrap()
}

/// Press Tab (`forward`) or Shift+Tab: the keystroke goes through the app's
/// keymap (gpui-component's Root moves the focus over the stops of the frame
/// on screen), so whatever watches the key sees it too.
fn tab(cx: &mut VisualTestAppContext, window: AnyWindowHandle, forward: bool) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.simulate_keystrokes(window, if forward { "tab" } else { "shift-tab" });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.run_until_parked();
}

/// Draws the nav's GitHub cells for the scenario, and stops on drop.
struct GithubNav;

impl GithubNav {
    fn shown() -> Self {
        e2e::set_github_nav(true);
        Self
    }
}

impl Drop for GithubNav {
    fn drop(&mut self) {
        e2e::set_github_nav(false);
    }
}

/// Press and release each of the whitespace-separated `keys`. Enter and
/// Space press a focused element on their release (gpui's keyboard click),
/// which `simulate_keystrokes` alone does not send.
pub(crate) fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, keys: &str) {
    for key in keys.split_whitespace() {
        cx.simulate_keystrokes(window, key);
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
    }
}

pub fn scenario_keyboard_nav(cx: &mut VisualTestAppContext) {
    // Home's reads must not reach GitHub; every one of them fails here.
    let _gh = OfflineGh::with_script("#!/bin/sh\necho offline >&2\nexit 1\n");
    let _nav = GithubNav::shown();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.run_until_parked();

    // Tab, from the window, reaches the nav's selected cell: it is a Tab
    // stop (moving focus as Tab does, which Tier A cannot press). The
    // window's other stops come first, so look a few presses ahead.
    let reached = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            (0..40).find_map(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                app.read(cx).mode_nav_focused_for_e2e(window)
            })
        })
        .unwrap();
    assert_eq!(reached, Some(0), "Tab reaches the selected mode cell");

    // The workspace-mode nav: the arrows only move, as entering PRs or
    // Issues starts a read; Enter / Space enter. Its PRs / Issues cells are
    // drawn whether or not this machine has a `gh` (#960 review).
    let focus_nav = |cx: &mut VisualTestAppContext, slot| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(slot, window, cx))
        })
        .unwrap();
    };
    focus_nav(cx, 0);
    keys(cx, window, "right");
    assert_eq!(
        mode(cx, &app),
        WorkspaceMode::Graph,
        "an arrow does not enter a mode"
    );
    keys(cx, window, "enter");
    assert_eq!(mode(cx, &app), WorkspaceMode::Prs, "Enter enters it");
    keys(cx, window, "home space");
    assert_eq!(mode(cx, &app), WorkspaceMode::Graph, "Home, then Space");

    // #960 review: the Tab stop follows the cell the arrows moved to, so
    // Tab and Shift+Tab leave the list in one press from there; coming back
    // in lands on the selected cell.
    for forward in [true, false] {
        focus_nav(cx, 0);
        keys(cx, window, "right right");
        assert_eq!(nav_focus(cx, &app, window), Some(2));
        tab(cx, window, forward);
        assert_eq!(
            nav_focus(cx, &app, window),
            None,
            "one press leaves the list (forward={forward})"
        );
        tab(cx, window, !forward);
        assert_eq!(
            nav_focus(cx, &app, window),
            Some(0),
            "back in, on the selected cell (forward={forward})"
        );
    }

    // Enter on a cell is the cell's alone: with a notice up, it does not
    // also reach the Enter that confirms the modal.
    app.update(cx, |app, cx| {
        e2e::deliver_app_notice(app, "a notice");
        cx.notify();
    });
    cx.run_until_parked();
    focus_nav(cx, 0);
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| e2e::app_notice_message(app.read(cx)).is_some()),
        "Enter on the Graph cell does not confirm the notice"
    );
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().unwrap();
        root.focus(window, cx);
    })
    .unwrap();
    keys(cx, window, "escape");
    assert!(cx.read(|cx| e2e::app_notice_message(app.read(cx)).is_none()));

    // A pointer click leaves the focus where a click on the window puts it,
    // so the arrows keep doing what they did (PR mode's pane cycling). The
    // Graph cell is the nav's first, at its left edge. From PRs, so the
    // click is seen to land on it.
    focus_nav(cx, 0);
    keys(cx, window, "right enter");
    assert_eq!(mode(cx, &app), WorkspaceMode::Prs);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let nav = e2e::control_bounds(window.window_id(), "sidebar-mode-nav").unwrap();
    let graph = gpui::point(nav.origin.x + gpui::px(24.), nav.center().y);
    cx.simulate_click(window, graph, gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        mode(cx, &app),
        WorkspaceMode::Graph,
        "the click is the Graph cell's"
    );
    assert!(
        root_focused(cx, &app, window),
        "the cell clicked gives the focus back"
    );

    // Home's switch selects as the arrows move: it only shows lists already
    // read, so moving through it reads nothing.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    let reads = cx.read(|cx| app.read(cx).home_reads_for_e2e());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_home_pane_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "left");
    assert_eq!(pane(cx, &app), HomePane::Repos, "no wrap at the first cell");
    keys(cx, window, "right");
    assert_eq!(pane(cx, &app), HomePane::Prs);
    keys(cx, window, "end");
    assert_eq!(pane(cx, &app), HomePane::Issues);
    keys(cx, window, "right");
    assert_eq!(pane(cx, &app), HomePane::Issues, "no wrap at the last cell");
    keys(cx, window, "home");
    assert_eq!(pane(cx, &app), HomePane::Repos);
    keys(cx, window, "right right left");
    assert_eq!(pane(cx, &app), HomePane::Prs);
    assert_eq!(
        cx.read(|cx| app.read(cx).home_reads_for_e2e()),
        reads,
        "switching panes from the keyboard reads nothing"
    );

    // Keys typed into a field are the field's: with Home's search focused the
    // switch is not on the focus path, so the arrows (and Enter, an IME's
    // commit included) do not reach it. Enter itself is not pressed here: the
    // harness types its "\n" into the single-line field, which a real Enter
    // does not.
    cx.update_window(window, |_, window, cx| {
        let filter = app
            .read(cx)
            .home_github
            .filter
            .clone()
            .expect("Home's search");
        filter.update(cx, |input, cx| input.focus(window, cx));
    })
    .unwrap();
    keys(cx, window, "right end left");
    assert_eq!(pane(cx, &app), HomePane::Prs);

    unmount(cx, app, window);
}
