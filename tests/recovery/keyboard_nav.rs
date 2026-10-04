//! Keyboard paths of Kagi's tab lists (#944), Tier A.
//!
//! The test dispatcher cannot press Tab or prove focus-visible, so a cell is
//! focused through a hook and the keys are pressed on it; what each key does
//! is checked. Rows are covered in `home_work` (Enter / Space on a row).

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::workspace_mode::WorkspaceMode;
use kagi::ui::{e2e, theme, KagiApp};

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

/// Which edge of a cell [`ring_keeps_geometry`] measures from.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Left,
}

/// #960 review: the ring is a fixed 2px taken out of the rem-scaled padding,
/// so the label's inset is `offset × theme::scaled(1.0)` from the measured
/// outer edge (within GPUI's half-pixel layout rounding). Keyboard focus
/// colours the ring but must never move or resize the label. Subtracting
/// the ring before scaling displaced the label past that rounding tolerance.
fn ring_keeps_geometry(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    (outer, label, edge, offset): (&str, &str, Edge, f32),
    focus: impl Fn(&mut VisualTestAppContext),
) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let measure = |cx: &mut VisualTestAppContext| {
        // The zoom reaches the layout through the rem size set at the top of
        // the render, so draw twice before measuring.
        for _ in 0..2 {
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
        }
        let at = |name: &str| {
            e2e::control_bounds(window.window_id(), name)
                .unwrap_or_else(|| panic!("{name} is drawn"))
        };
        (at(outer), at(label))
    };
    for zoom in [1.0, 0.7, 1.5] {
        theme::set_zoom(zoom);
        cx.update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
        })
        .unwrap();
        let (cell, text) = measure(cx);
        let inset = f32::from(match edge {
            Edge::Top => text.origin.y - cell.origin.y,
            Edge::Left => text.origin.x - cell.origin.x,
        });
        let physical_scale = theme::scaled(1.0);
        assert!(
            (inset - offset * physical_scale).abs() <= 0.5,
            "{label} at {zoom}×: {inset}px from {outer}'s edge, {offset} × {physical_scale} = {}px expected",
            offset * physical_scale
        );
        focus(cx);
        // A key with no binding makes the last input a key, so the ring is
        // drawn (`focus_visible`) when the cell is measured again.
        cx.simulate_keystrokes(window, "f19");
        assert_eq!(
            measure(cx),
            (cell, text),
            "{outer} at {zoom}×: taking focus moves or resizes nothing"
        );
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
    // An arrow moves the focus to PRs without selecting it; then the focus
    // goes elsewhere another way than Tab (a pointer click on another
    // control, here the window's own focus). The next Tab must land on the
    // selected cell again, not on the cell the arrow left (#968).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "right");
    assert_eq!(nav_focus(cx, &app, window), Some(1), "precondition: on PRs");
    assert_eq!(mode(cx, &app), WorkspaceMode::Graph, "an arrow only moves");
    let reached = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            // The frame after the focus left, as on screen.
            window.draw(cx).clear();
            (0..40).find_map(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                app.read(cx).mode_nav_focused_for_e2e(window)
            })
        })
        .unwrap();
    assert_eq!(
        reached,
        Some(0),
        "after the focus left another way, Tab lands on the selected mode cell"
    );
    // The nav's top margin (`mt_1`) and the cell's padding, 4 + 4 at 1.0.
    let nav = (
        "sidebar-mode-nav",
        "sidebar-mode-graph-label",
        Edge::Top,
        8.,
    );
    ring_keeps_geometry(cx, &app, window, nav, |cx| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx))
        })
        .unwrap();
    });

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
    let switch = ("home-pane-repos", "home-pane-repos-label", Edge::Left, 12.);
    ring_keeps_geometry(cx, &app, window, switch, |cx| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.focus_home_pane_for_e2e(0, window, cx))
        })
        .unwrap();
    });

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

    // The repository tab strip (#959): the arrows only move between tabs —
    // switching reads the repository again — and Enter / Space switch.
    let second = build_fixture();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(second.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    let (repos, home_front) = cx.read(|cx| {
        let app = app.read(cx);
        (app.tabs.len(), app.home.is_some_and(|home| home.front))
    });
    assert!(home_front, "Home is in front");
    let strip_focus = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).tab_strip_focused_for_e2e(window)
        })
        .unwrap()
    };
    // From the window, one Tab reaches the strip on the tab in front, and
    // the next leaves it: a tab's × is not a stop of its own.
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();
    tab(cx, window, true);
    assert_eq!(
        strip_focus(cx),
        Some(repos),
        "Tab lands on the tab in front"
    );
    tab(cx, window, true);
    assert_eq!(strip_focus(cx), None, "one Tab leaves the strip");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "right");
    assert_eq!(strip_focus(cx), Some(1));
    assert!(
        cx.read(|cx| app.read(cx).home.is_some_and(|home| home.front)),
        "an arrow does not switch"
    );
    keys(cx, window, "enter");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.home.is_some_and(|home| home.front), "Enter switches");
        assert_eq!(app.active_tab, 1);
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "end");
    assert_eq!(strip_focus(cx), Some(repos), "Home is the last tab");
    keys(cx, window, "space");
    assert!(
        cx.read(|cx| app.read(cx).home.is_some_and(|home| home.front)),
        "Space brings Home to the front"
    );
    // The key pressed on Home's cell leaves the focus there, as Enter on a
    // repository's cell does, so ← goes on working (#961 review).
    assert_eq!(
        strip_focus(cx),
        Some(repos),
        "the focus stays on Home's cell after Space"
    );
    keys(cx, window, "left");
    assert_eq!(
        strip_focus(cx),
        Some(repos - 1),
        "← moves to the last repository"
    );

    // With a repository in front and Home behind it, the + after the tabs
    // would bring Home forward when pressed: the Tab that leaves the strip
    // must not stop on it (#961 review).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).home.is_some_and(|home| !home.front)));
    // Tab out and Shift+Tab back: the strip's stop is the tab in front.
    tab(cx, window, true);
    tab(cx, window, false);
    assert_eq!(
        strip_focus(cx),
        Some(0),
        "the strip's stop is the tab in front"
    );
    tab(cx, window, true);
    assert_eq!(strip_focus(cx), None, "one Tab leaves the strip");
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| app.read(cx).home.is_some_and(|home| !home.front)),
        "the Tab after the strip is not the + (Enter would bring Home forward)"
    );

    // Closing the tab whose cell holds the focus hands it to the window
    // instead of leaving it on a cell nothing draws (#961 review).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(repos, window, cx))
    })
    .unwrap();
    assert_eq!(strip_focus(cx), Some(repos), "precondition: Home's cell");
    app.update(cx, |app, cx| app.close_home_tab(cx));
    cx.run_until_parked();
    assert_eq!(strip_focus(cx), None);
    assert!(
        root_focused(cx, &app, window),
        "the closed Home tab's focus goes to the window"
    );
    // ⌘W with the focus arrowed to another tab's cell closes the tab in
    // front, not that one; the cells after it move up a slot, and the focus
    // and the arrowed-to cell move with their tab (#961 review): Enter then
    // switches to the tab that had the focus.
    assert_eq!(
        cx.read(|cx| app.read(cx).tabs.len()),
        2,
        "two repository tabs"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).active_tab),
        0,
        "the first in front"
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "right");
    assert_eq!(strip_focus(cx), Some(1), "precondition: on the second tab");
    app.update(cx, |app, cx| app.close_tab(0, cx));
    cx.run_until_parked();
    assert_eq!(
        strip_focus(cx),
        Some(0),
        "the focus moved with the second tab into the first slot"
    );
    keys(cx, window, "enter");
    let (tabs, active, path) = cx.read(|cx| {
        let app = app.read(cx);
        (
            app.tabs.len(),
            app.active_tab,
            app.tabs.first().map(|tab| tab.path.clone()),
        )
    });
    assert_eq!((tabs, active), (1, 0));
    assert_eq!(
        path.map(|p| p.canonicalize().unwrap()),
        Some(second.path().canonicalize().unwrap()),
        "the tab left is the one the focus was on"
    );
    let third = build_fixture();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(third.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    // The same for a tab that is not the last: its cell is drawn again, for
    // the tab after it, so the focus must not pass to that tab (#961 review).
    assert!(repos >= 2, "two repository tabs");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    assert_eq!(
        strip_focus(cx),
        Some(0),
        "precondition: the first tab's cell"
    );
    app.update(cx, |app, cx| app.close_tab(0, cx));
    cx.run_until_parked();
    assert_eq!(
        strip_focus(cx),
        None,
        "the tab that moved into the closed tab's cell does not get its focus"
    );
    assert!(
        root_focused(cx, &app, window),
        "the closed first tab's focus goes to the window"
    );

    // A repository opened while the focus is on Home's cell (Home behind)
    // is added before Home: Home's cell, its handle and the focus move to
    // the next slot with it, and Enter brings Home forward, not the new tab
    // (#961 review: the strip's cells are keyed by tab, not by slot).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_tab_strip_for_e2e(0, window, cx))
    })
    .unwrap();
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| app.read(cx).home.is_some_and(|home| !home.front)),
        "precondition: Home behind the repository"
    );
    let before = cx.read(|cx| app.read(cx).tabs.len());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_tab_strip_for_e2e(before, window, cx)
        })
    })
    .unwrap();
    assert_eq!(strip_focus(cx), Some(before), "precondition: Home's cell");
    let fourth = build_fixture();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(fourth.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), before + 1);
    assert_eq!(
        strip_focus(cx),
        Some(before + 1),
        "the focus stays on Home's cell, now after the new tab"
    );
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| app.read(cx).home.is_some_and(|home| home.front)),
        "Enter on the focused cell brings Home forward, not the new tab"
    );

    unmount(cx, app, window);
}
