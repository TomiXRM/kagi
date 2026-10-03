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
    let _shell = crate::gui_isolation::StandInShell::install();
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

/// Where the focus is, as #974 needs to see it.
#[derive(Debug, PartialEq, Eq)]
enum Held {
    Terminal,
    /// Settings' trap container itself: no control, so no ring.
    Trap,
    /// A control inside Settings (`editor`: the Analyze-ignore editor).
    InSettings {
        editor: bool,
    },
    Elsewhere,
}

fn held(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) -> Held {
    cx.update_window(window, |_, window, cx| {
        let app = app.read(cx);
        if app.terminal_focused_for_e2e(window, cx) {
            Held::Terminal
        } else if app.settings_trap_focused_for_e2e(window) {
            Held::Trap
        } else if app.settings_trap_contains_focus_for_e2e(window, cx) {
            let editor = app
                .analyze_ignore_input
                .as_ref()
                .is_some_and(|input| input.read(cx).focus_handle(cx).contains_focused(window, cx));
            Held::InSettings { editor }
        } else {
            Held::Elsewhere
        }
    })
    .unwrap()
}

/// #974: Settings opened over a focused terminal keeps Tab / Shift+Tab
/// inside its panel — the terminal's shell never holds the focus, so no key
/// reaches it — and Escape gives the focus back to the terminal. Opened by
/// pointer, the focus lands on the trap container, which draws no ring.
pub fn scenario_settings_focus_trap(cx: &mut VisualTestAppContext) {
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let start_terminal = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.bottom_panel_open = true;
                app.bottom_tab = kagi::ui::BottomTab::Terminal;
                app.ensure_terminal(window, cx);
            })
        })
        .unwrap();
        draw(cx, window);
        assert_eq!(
            held(cx, &app, window),
            Held::Terminal,
            "precondition: the terminal holds the focus"
        );
    };
    start_terminal(cx);

    // Opened from the menu (the keyboard / palette route).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    assert_eq!(
        held(cx, &app, window),
        Held::Trap,
        "settings-open-takes-focus: opening Settings must move the focus into it"
    );

    // Tab walks the controls up to the Analyze-ignore editor, never leaving
    // the panel.
    let editor_text = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            let input = app.read(cx).analyze_ignore_input.clone().expect("editor");
            input.read(cx).value().to_string()
        })
    };
    let tab_to_editor = |cx: &mut VisualTestAppContext| {
        let mut steps = 0;
        loop {
            keys(cx, window, "tab");
            steps += 1;
            let now = held(cx, &app, window);
            assert!(
                matches!(now, Held::InSettings { .. }),
                "settings-tab-trapped: Tab #{steps} left Settings: {now:?}"
            );
            if now == (Held::InSettings { editor: true }) {
                return steps;
            }
            assert!(steps < 60, "Tab never reached the Analyze-ignore editor");
        }
    };
    let steps = tab_to_editor(cx);
    assert!(
        steps >= 3,
        "Tab must visit Settings' controls, took {steps}"
    );
    let text = editor_text(cx);

    // The editor never keeps Tab: it is an auto-grow input, which gpui-
    // component does not indent, so Tab / Shift+Tab fall through to Root and
    // move on (to Save), and the walk comes back to the editor after one
    // full turn of the trap (#977). A code editor or plain multi-line input
    // here would take Tab for indenting and stop the walk.
    keys(cx, window, "tab");
    assert_eq!(
        held(cx, &app, window),
        Held::InSettings { editor: false },
        "analyze-ignore-tab-leaves: Tab must move from the editor to Save"
    );
    let turn = 1 + tab_to_editor(cx);
    assert!(
        turn > steps,
        "analyze-ignore-tab-cycles: a full turn ({turn}) must pass every stop"
    );
    keys(cx, window, "shift-tab");
    assert_eq!(
        held(cx, &app, window),
        Held::InSettings { editor: false },
        "analyze-ignore-shift-tab-leaves: Shift+Tab must move out of the editor"
    );
    keys(cx, window, "tab");
    assert_eq!(held(cx, &app, window), Held::InSettings { editor: true });
    assert_eq!(
        editor_text(cx),
        text,
        "analyze-ignore-tab-no-indent: Tab / Shift+Tab must not edit the text"
    );

    // Both ends wrap: from the container, Shift+Tab lands on the panel's
    // last stop (past the editor); Tab from there wraps to the first one.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_settings_trap_for_e2e(window, cx))
    })
    .unwrap();
    for (key, what) in [
        ("shift-tab", "wrap to the last stop"),
        ("tab", "wrap to the first stop"),
        ("shift-tab", "back to the last stop"),
    ] {
        keys(cx, window, key);
        assert_eq!(
            held(cx, &app, window),
            Held::InSettings { editor: false },
            "settings-tab-wraps: {key} must {what} inside Settings"
        );
    }

    keys(cx, window, "escape");
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-escape-closes: Escape must close Settings"
    );
    draw(cx, window);
    assert_eq!(
        held(cx, &app, window),
        Held::Terminal,
        "settings-escape-returns-focus: Escape must give the focus back to the terminal"
    );

    // Opened by pointer: the focus is on the container, not on a control
    // that would show its ring for a mouse user.
    let button = e2e::control_bounds(window.window_id(), "tb-settings").expect("Settings button");
    cx.simulate_click(window, button.center(), Modifiers::none());
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "the click opens Settings"
    );
    assert_eq!(
        held(cx, &app, window),
        Held::Trap,
        "settings-pointer-open: a pointer open must focus the ring-less container"
    );
    // The click itself took the focus to the toolbar button before Settings
    // opened, so that is where Escape returns it (the control that opened
    // Settings, as #817 returns focus to wherever it was).
    keys(cx, window, "escape");
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    assert_eq!(
        held(cx, &app, window),
        Held::Elsewhere,
        "settings-pointer-close: Escape returns the focus out of Settings"
    );

    // #976 review: with a modal open (Stash, which has a message field),
    // Settings — drawn behind the modal layer — must not open and take the
    // focus from the modal in front.
    std::fs::write(repo.join("README.md"), "dirty for the stash modal\n").unwrap();
    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).stash_push_modal().is_some()),
        "precondition: the Stash modal is open"
    );
    let focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| window.focused(cx))
            .unwrap()
    };
    let before = focused(cx);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-not-behind-modal: Settings opened behind the open modal"
    );
    assert_eq!(
        focused(cx),
        before,
        "settings-not-behind-modal: opening Settings moved the modal's focus"
    );
    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_none()));

    // A modal that arrives while Settings is open (an async plan landing)
    // is in front: Settings yields, and the focus leaves its trap so the
    // modal's keys work — Escape closes the modal.
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
        "precondition: the open theme Select popup holds focus outside the trap"
    );
    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-yields-to-modal: Settings stayed open behind the arriving modal"
    );
    assert!(
        !matches!(held(cx, &app, window), Held::Trap | Held::InSettings { .. }),
        "settings-yields-to-modal: the focus stayed in Settings' trap"
    );
    keys(cx, window, "escape");
    assert!(
        cx.read(|cx| app.read(cx).stash_push_modal().is_none()),
        "settings-select-yields-to-modal: Escape must close the arrived modal even with the theme popup open"
    );

    // A fieldless modal cannot steal focus during input initialization. Its
    // Escape has to be routed away from the Select popup by the yield itself.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    let picker = e2e::control_bounds(window.window_id(), "settings-theme-select")
        .expect("Settings theme picker");
    cx.simulate_click(window, picker.center(), Modifiers::none());
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            let app = app.read(cx);
            let select = app.theme_select.clone().expect("theme picker");
            select.read(cx).focus_handle(cx).is_focused(window)
                && !app.settings_trap_focused_for_e2e(window)
        })
        .unwrap(),
        "precondition: the fieldless modal arrives while the theme Select holds focus"
    );
    app.update(cx, |app, cx| app.open_push_modal(cx));
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|root| root.is_focused(window))
        })
        .unwrap(),
        "settings-select-fieldless-modal-focus: the popup must yield focus to the modal's root"
    );
    keys(cx, window, "escape");
    assert!(
        cx.read(|cx| app.read(cx).push_modal().is_none()),
        "settings-select-fieldless-modal-escape: Escape must close the arrived fieldless modal"
    );

    // The Commit Panel's plan confirmation (its own storage) is in front
    // too: Settings does not open over it.
    std::fs::write(
        repo.join("conflict.txt"),
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\n",
    )
    .unwrap();
    let added = std::process::Command::new("git")
        .current_dir(&repo)
        .args(["add", "conflict.txt"])
        .status()
        .unwrap();
    assert!(added.success());
    app.update(cx, |app, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo.clone(), cx);
    });
    draw(cx, window);
    app.update(cx, |app, cx| {
        let owner = app.active_session().expect("owner");
        let panel = app.ui().commit_panel.clone().expect("Commit Panel");
        panel.update(cx, |panel, _| panel.state.commit_msg = "probe".into());
        app.open_commit_plan_modal(owner, cx);
    });
    draw(cx, window);
    let plan_open = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            (app.read(cx).ui().commit_panel.as_ref())
                .is_some_and(|panel| panel.read(cx).state.plan_modal.is_some())
        })
    };
    assert!(
        plan_open(cx),
        "precondition: the conflict marker blocks the commit plan"
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-not-behind-commit-plan: Settings opened behind the commit plan"
    );
    let open_settings = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            })
        })
        .unwrap();
        draw(cx, window);
    };
    let open_home = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.open_home_tab(window, cx))
        })
        .unwrap();
        draw(cx, window);
    };
    // The same plan kept behind Home is not drawn: it does not block Settings.
    open_home(cx);
    open_settings(cx);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "settings-over-home-with-hidden-plan: a plan not drawn must not block Settings"
    );
    keys(cx, window, "escape");
    app.update(cx, |app, cx| {
        app.close_home_tab(cx);
        let panel = app.ui().commit_panel.clone().expect("Commit Panel");
        panel.update(cx, |panel, _| panel.state.plan_modal = None);
    });
    draw(cx, window);

    // New Tab (Home) while Settings is open closes Settings first: the
    // focus Home moves to the root must not cycle outside a trap that
    // still covers the screen.
    open_settings(cx);
    assert_eq!(
        held(cx, &app, window),
        Held::Trap,
        "precondition: Settings open"
    );
    open_home(cx);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none() && app.read(cx).home_in_front()),
        "settings-closes-for-home: Home opened under a still-open Settings"
    );
    for _ in 0..2 {
        keys(cx, window, "tab");
        assert_eq!(
            held(cx, &app, window),
            Held::Elsewhere,
            "settings-closes-for-home: Tab must move over Home, not Settings or the terminal"
        );
    }
    app.update(cx, |app, cx| app.close_home_tab(cx));
    draw(cx, window);

    // About (or Keyboard Shortcuts) from the native menu replaces Settings:
    // the focus leaves the unmounted trap for the root — not for Settings'
    // return target, the terminal, where Escape (bound `!Terminal`) would
    // not reach the overlay — so Escape closes it.
    for (command, what) in [
        ("app.about", "About"),
        ("help.shortcuts", "Keyboard Shortcuts"),
    ] {
        start_terminal(cx);
        open_settings(cx);
        assert_eq!(
            held(cx, &app, window),
            Held::Trap,
            "precondition: Settings open"
        );
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.handle_menu_command(command, window, cx))
        })
        .unwrap();
        draw(cx, window);
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_some()),
            "{what} replaces Settings"
        );
        keys(cx, window, "escape");
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_none()),
            "settings-replaced-overlay-escape: Escape must close {what} opened over Settings"
        );
    }

    // (b) Backstop: whatever moves the focus out of an open Settings (here
    // the root, focused directly) — the next frame puts it back in the trap.
    open_settings(cx);
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        window.focus(&root, cx);
    })
    .unwrap();
    draw(cx, window);
    assert_eq!(
        held(cx, &app, window),
        Held::Trap,
        "settings-focus-backstop: a focus moved out of Settings must return to its trap"
    );

    // (a) A command acting behind Settings (File → Open in Terminal) closes
    // Settings first; the terminal it opens holds the focus.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("file.openInTerminal", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-closes-for-command: Open in Terminal left Settings open"
    );
    assert_eq!(
        held(cx, &app, window),
        Held::Terminal,
        "settings-closes-for-command: the opened terminal must hold the focus"
    );

    // Cmd+J and View → Toggle Terminal share the bottom-panel action, not
    // the Open in Terminal command. Closing either direction must discard
    // Settings' saved terminal focus, even when the panel becomes hidden.
    for panel_was_open in [true, false] {
        if panel_was_open {
            start_terminal(cx);
        } else {
            app.update(cx, |app, _| app.bottom_panel_open = false);
            draw(cx, window);
        }
        open_settings(cx);
        assert_eq!(held(cx, &app, window), Held::Trap);
        keys(cx, window, "cmd-j");
        draw(cx, window);
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_none()),
            "settings-toggle-terminal: Cmd+J must close Settings (panel was open: {panel_was_open})"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).bottom_panel_open),
            !panel_was_open,
            "settings-toggle-terminal: Cmd+J must toggle the panel"
        );
        keys(cx, window, "escape");
        for _ in 0..3 {
            draw(cx, window);
        }
        assert_ne!(
            held(cx, &app, window),
            Held::Terminal,
            "settings-toggle-terminal: Escape must not restore a hidden terminal (panel was open: {panel_was_open})"
        );
        assert!(
            cx.update_window(window, |_, window, cx| {
                app.read(cx)
                    .root_focus
                    .as_ref()
                    .is_some_and(|root| root.is_focused(window))
            })
            .unwrap(),
            "settings-toggle-terminal: keys must land on the visible root after Cmd+J"
        );
    }

    // The platform menu / palette calls the command id directly instead of
    // dispatching the GPUI action. It must use the same toggle transition.
    start_terminal(cx);
    open_settings(cx);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.toggleTerminal", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none() && !app.read(cx).bottom_panel_open),
        "settings-toggle-terminal-menu: the command route must close Settings and the panel"
    );
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|root| root.is_focused(window))
        })
        .unwrap(),
        "settings-toggle-terminal-menu: focus must stay on the visible root"
    );

    // Settings' return target belongs to its opening screen. Switch to a
    // second repository without a Window-bearing command (as an async tab
    // activation can), then close Settings: the old terminal is still alive
    // but cannot own keys on the newly active repository.
    let other_fixture = build_fixture();
    let other = other_fixture.path().canonicalize().unwrap();
    assert!(app.update(cx, |app, cx| app.open_repository(other.clone(), cx)));
    draw(cx, window);
    app.update(cx, |app, cx| {
        app.open_file_history(std::path::PathBuf::from("README.md"), None, cx);
    });
    draw(cx, window);
    let other_history = cx
        .read(|cx| app.read(cx).ui().file_history.clone())
        .expect("second repository File History");
    assert!(
        cx.read(|cx| other_history.read(cx).data.commit_count()) >= 2,
        "precondition: the destination has history to step"
    );
    assert_eq!(cx.read(|cx| other_history.read(cx).data.selected), 0);
    assert!(app.update(cx, |app, cx| app.open_repository(repo.clone(), cx)));
    draw(cx, window);
    start_terminal(cx);
    open_settings(cx);
    assert_eq!(held(cx, &app, window), Held::Trap);
    assert!(app.update(cx, |app, cx| app.open_repository(other.clone(), cx)));
    draw(cx, window);
    keys(cx, window, "escape");
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "settings-tab-switch-escape: Escape closes Settings on the new tab"
    );
    assert!(
        cx.update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().expect("root focus");
            root.is_focused(window)
        })
        .unwrap(),
        "settings-tab-switch-focus: closing Settings must not restore the old tab's terminal"
    );
    keys(cx, window, "down");
    assert_eq!(
        cx.read(|cx| other_history.read(cx).data.selected),
        1,
        "settings-tab-switch-keys: Down must reach the new tab's File History"
    );
    drop(other_history);

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_focus_trap");
}

/// #976: a focus handle can outlive its pane. Settings must not restore it
/// after the pane disappears, even when the repository session is unchanged;
/// a restore while the pane is *still closing* must also recover on unmount.
pub fn scenario_settings_hidden_return_target(cx: &mut VisualTestAppContext) {
    use std::time::{Duration, Instant};

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).view().rows.len()) >= 2,
        "precondition: Graph has at least two commits"
    );
    let selected = |cx: &mut VisualTestAppContext| cx.read(|cx| app.read(cx).ui().selected);
    let root_focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|root| root.is_focused(window))
        })
        .unwrap()
    };
    let focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window
                .focused(cx)
                .expect("focus handle before opening Settings")
        })
        .unwrap()
    };
    let in_workspace = |cx: &mut VisualTestAppContext, focus: &gpui::FocusHandle| {
        cx.update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().expect("workspace root");
            root.contains(focus, window)
        })
        .unwrap()
    };
    let open_settings = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            })
        })
        .unwrap();
        draw(cx, window);
    };
    let finish = |cx: &mut VisualTestAppContext, label: &str| {
        keys(cx, window, "escape");
        draw(cx, window);
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_none()),
            "{label}: Escape closes Settings"
        );
        keys(cx, window, "down");
        assert_eq!(
            selected(cx),
            Some(1),
            "{label}: raw Down must advance the Graph after Settings closes"
        );
        assert!(root_focused(cx), "{label}: focus returns to the drawn root");
    };

    let t0 = Instant::now();
    e2e::set_panel_motion_clock(Some(t0));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx))
    })
    .unwrap();
    draw(cx, window);
    assert!(!root_focused(cx), "precondition: mode-nav cell owns focus");
    let sidebar_focus = focused(cx);
    assert!(
        in_workspace(cx, &sidebar_focus),
        "precondition: mode-nav focus belongs to the visible workspace"
    );
    open_settings(cx);
    keys(cx, window, "cmd-b");
    assert!(
        cx.read(|cx| !app.read(cx).sidebar.visible),
        "Cmd+B must hide the sidebar while Settings remains open"
    );
    // Escape before the 150 ms close animation finishes. The return target
    // still belongs to the drawn workspace at the moment it is restored.
    e2e::set_panel_motion_clock(Some(t0 + Duration::from_millis(50)));
    draw(cx, window);
    assert!(
        in_workspace(cx, &sidebar_focus),
        "precondition: sidebar control remains drawn while closing"
    );
    keys(cx, window, "escape");
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "mid-close: Escape closes Settings before the sidebar unmounts"
    );
    assert_eq!(
        focused(cx),
        sidebar_focus,
        "mid-close: Settings restores the still-drawn sidebar control"
    );
    e2e::set_panel_motion_clock(Some(t0 + Duration::from_millis(300)));
    draw(cx, window);
    draw(cx, window);
    assert!(
        !in_workspace(cx, &sidebar_focus),
        "precondition: the sidebar control unmounts when the animation ends"
    );
    keys(cx, window, "down");
    assert_eq!(
        selected(cx),
        Some(1),
        "mid-close: raw Down reaches Graph after the restored control unmounts"
    );
    assert!(
        root_focused(cx),
        "mid-close: focus recovers to the drawn root"
    );

    // Reopen the sidebar so the second leg can focus an inspector control.
    keys(cx, window, "cmd-b");
    assert!(cx.read(|cx| app.read(cx).sidebar.visible));
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    e2e::set_panel_motion_clock(Some(t0 + Duration::from_millis(600)));
    draw(cx, window);
    let message = e2e::control_bounds(window.window_id(), "inspector-message-scroll")
        .expect("the inspector's selectable commit message is drawn");
    cx.simulate_click(
        window,
        gpui::point(
            message.origin.x + gpui::px(24.),
            message.origin.y + gpui::px(12.),
        ),
        Modifiers::none(),
    );
    draw(cx, window);
    assert!(
        !root_focused(cx),
        "precondition: clicking the inspector's TextView focuses it"
    );
    let inspector_focus = focused(cx);
    assert!(
        in_workspace(cx, &inspector_focus),
        "precondition: the inspector focus belongs to the visible workspace"
    );
    open_settings(cx);
    keys(cx, window, "cmd-alt-b");
    assert!(
        cx.read(|cx| !app.read(cx).inspector_visible),
        "Cmd+Option+B must hide commit details while Settings remains open"
    );
    e2e::set_panel_motion_clock(Some(t0 + Duration::from_millis(900)));
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "commit-details toggle must leave Settings open"
    );
    assert!(
        !in_workspace(cx, &inspector_focus),
        "precondition: the captured inspector focus is no longer drawn"
    );
    finish(cx, "hidden-commit-details-return");

    e2e::set_panel_motion_clock(None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_hidden_return_target");
}
