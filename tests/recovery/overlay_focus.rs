//! #817 / #812: a menu overlay that took focus (Command Palette input,
//! Settings' own controls) must hand it back when it closes, or keys land on
//! an undrawn element and reach neither the modal slot nor the lists.
//!
//! Every key here is a raw keystroke delivered wherever the app left focus.
//! The helpers in `operations.rs` re-focus the root before each key, which is
//! exactly what would hide this bug, so they are not used.

use gpui::{AnyWindowHandle, Entity, Focusable, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, FooterStatus, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

/// Draw, then deliver `keys` with no test-side focus change.
fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, keys: &str) {
    draw(cx, window);
    cx.simulate_keystrokes(window, keys);
    cx.run_until_parked();
}

fn push_refusals(repo: &std::path::Path) -> usize {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|e| e.op == "push" && matches!(e.outcome, OpOutcome::Refused { .. }))
        .count()
}

/// Cmd+P → "push" → Enter: the palette runs `repo.push`, which opens the
/// blocked plan (the fixture has no remote).
fn open_push_from_palette(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    keys(cx, window, "cmd-p");
    keys(cx, window, "p u s h");
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| app
            .read(cx)
            .push_modal()
            .is_some_and(|m| !m.plan.blockers.is_empty())),
        "the palette's Push must open the blocked push plan"
    );
}

pub fn scenario_palette_push_modal_keys(cx: &mut VisualTestAppContext) {
    let _ports = crate::gui_isolation::PortStore::keep();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    // The real launch opens the bottom panel on the Terminal tab and starts
    // the shell, which takes focus (`open_main_window` → `ensure_terminal`).
    // Escape is bound `!Terminal`, so this is the focus the palette returns to.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.bottom_panel_open = true;
            app.bottom_tab = kagi::ui::BottomTab::Terminal;
            app.ensure_terminal(window, cx);
        })
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().expect("root focus");
            window.focused(cx).is_some() && !root.is_focused(window)
        })
        .unwrap(),
        "precondition: the started terminal holds focus, as at launch"
    );

    // Enter refuses the blocked plan: the modal closes, the refusal is durable
    // and the footer carries it.
    open_push_from_palette(cx, &app, window);
    keys(cx, window, "enter");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.push_modal().is_none(),
            "Enter must reach the push modal opened from the palette"
        );
        assert!(
            matches!(app.status_footer, FooterStatus::Failed(_)),
            "the refusal must reach the footer, got {:?}",
            app.status_footer
        );
    });
    assert_eq!(push_refusals(&repo), 1, "Enter must record one refusal");

    // Escape closes it without recording anything.
    open_push_from_palette(cx, &app, window);
    keys(cx, window, "escape");
    assert!(
        cx.read(|cx| app.read(cx).push_modal().is_none()),
        "Escape must close the push modal opened from the palette"
    );
    assert_eq!(push_refusals(&repo), 1, "Escape must not refuse or push");
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "a refused push mutates nothing"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS palette_push_modal_keys");
}

pub fn scenario_settings_close_returns_focus(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        app.open_file_history(std::path::PathBuf::from("README.md"), None, cx)
    });
    cx.run_until_parked();
    let history = cx
        .read(|cx| app.read(cx).ui().file_history.clone())
        .expect("File History opens");
    let selected = |cx: &mut VisualTestAppContext| cx.read(|cx| history.read(cx).data.selected);
    assert!(
        cx.read(|cx| history.read(cx).data.commit_count()) >= 2,
        "the fixture's README has two commits to step between"
    );
    // Precondition: ↓ / ↑ reach the list before Settings is involved.
    keys(cx, window, "down");
    assert_eq!(selected(cx), 1, "precondition: ↓ steps File History");
    keys(cx, window, "up");
    assert_eq!(selected(cx), 0);

    // #812's steps: open Settings the menu's way and use the theme picker (a
    // real click — what moves focus into Settings), then close it by each of
    // the reported routes and step the list with a raw arrow key.
    for (close_by, arrow, expected) in [("escape", "down", 1), ("×", "up", 0)] {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            })
        })
        .unwrap();
        draw(cx, window);
        let picker = e2e::control_bounds(window.window_id(), "settings-theme-select")
            .expect("Settings draws the theme picker");
        cx.simulate_click(window, picker.center(), Modifiers::none());
        draw(cx, window);
        assert!(
            cx.update_window(window, |_, window, cx| {
                let select = app.read(cx).theme_select.clone().expect("theme picker");
                select.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap(),
            "precondition: the picker click moves focus into Settings"
        );
        if close_by == "escape" {
            keys(cx, window, "escape");
        } else {
            // While the picker's list is open, a click outside it only
            // dismisses the list (the picker keeps focus), so × may take a
            // second click — as it does for a user.
            for _ in 0..2 {
                if cx.read(|cx| app.read(cx).menu_overlay.is_none()) {
                    break;
                }
                draw(cx, window);
                let close = e2e::control_bounds(window.window_id(), "settings-close")
                    .expect("Settings draws its × button");
                cx.simulate_click(window, close.center(), Modifiers::none());
                cx.run_until_parked();
            }
        }
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_none()),
            "{close_by} closes Settings"
        );

        keys(cx, window, arrow);
        assert_eq!(
            selected(cx),
            expected,
            "{arrow} right after closing Settings by {close_by} must reach File History"
        );
    }

    drop(history);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_close_returns_focus");
}

/// Scrolling over Settings must not reach the panes behind it — in every
/// workspace mode, not only Graph. A probe on the workspace root counts the
/// wheel events that get past the overlay. Precondition per mode: with
/// Settings closed the same wheel event does reach the workspace, so the
/// check can fail.
pub fn scenario_settings_scroll_stays_in_overlay(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    draw(cx, window);
    let centre = cx
        .update_window(window, |_, window, _| {
            let size = window.viewport_size();
            gpui::point(size.width * 0.5, size.height * 0.5)
        })
        .unwrap();
    let wheel = |cx: &mut VisualTestAppContext| {
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: centre,
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-200.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        draw(cx, window);
    };

    let modes: [(&str, fn(&mut KagiApp, &mut gpui::Context<KagiApp>)); 4] = [
        ("Graph", |app, cx| app.show_graph_mode(cx)),
        ("PRs", |app, cx| app.show_pr_mode(cx)),
        ("Issues", |app, cx| app.show_issues_mode(cx)),
        ("Editor", |app, cx| app.show_editor_mode(cx)),
    ];
    for (name, show) in modes {
        app.update(cx, |app, cx| {
            app.menu_overlay = None;
            show(app, cx);
        });
        draw(cx, window);

        let before = e2e::workspace_scrolls();
        wheel(cx);
        assert!(
            e2e::workspace_scrolls() > before,
            "precondition ({name}): with Settings closed the wheel reaches the workspace"
        );

        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            })
        })
        .unwrap();
        draw(cx, window);
        let before = e2e::workspace_scrolls();
        wheel(cx);
        assert_eq!(
            e2e::workspace_scrolls(),
            before,
            "settings-scroll-stays-in-overlay ({name}): a wheel over Settings reached the pane behind it"
        );
    }
    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        app.show_graph_mode(cx);
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_scroll_stays_in_overlay: Graph / PRs / Issues / Editor — the wheel over Settings never reaches the workspace");
}
