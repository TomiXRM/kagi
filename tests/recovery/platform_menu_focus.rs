//! #990: platform dropdown keyboard ownership, modal/input focus, and navigation.

use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use kagi::ui::commands::{BranchPickerMode, MenuOverlay};
use kagi::ui::{e2e, KagiApp};

use crate::macos::{build_fixture, git, repo_fingerprint, unmount};
use crate::platform_menu_scroll::mount_short;

fn overlay_key(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw before key");
    cx.simulate_keystrokes(win, key);
    cx.run_until_parked();
}

fn root_has_focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) -> bool {
    cx.update_window(win, |_, window, cx| {
        app.read(cx)
            .root_focus
            .as_ref()
            .is_some_and(|root| root.is_focused(window))
    })
    .expect("root focus probe")
}

/// #990 row 1: the menu takes Tab without moving through the hidden Settings
/// trap; Escape closes only the menu, then Settings returns to its opener.
pub fn scenario_platform_menu_settings_focus(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount_short(cx, &repo);
    cx.update_window(win, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root");
        window.focus(&root, cx);
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .settings_trap_contains_focus_for_e2e(window, cx)
        })
        .unwrap(),
        "Settings has focus before opening the menu"
    );
    let hidden_trap = cx
        .update_window(win, |_, window, cx| {
            window.focused(cx).expect("Settings trap")
        })
        .unwrap();
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx));
    });
    cx.dispatch_keystroke(win, gpui::Keystroke::parse("tab").unwrap());
    assert!(
        root_has_focus(cx, &app, win),
        "Tab during dropdown open moved focus behind it before the next render"
    );
    cx.run_until_parked();
    assert!(
        root_has_focus(cx, &app, win),
        "the dropdown releases the hidden trap"
    );
    for key in ["tab", "shift-tab"] {
        cx.update_window(win, |_, window, cx| {
            window.draw(cx).clear();
            // A focus event can precede the next render after the dropdown
            // opens. Keep the old trap focused to exercise capture, not just
            // the render-pass root-focus backstop.
            window.focus(&hidden_trap, cx);
            window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
            assert!(
                app.read(cx)
                    .root_focus
                    .as_ref()
                    .is_some_and(|root| root.is_focused(window)),
                "{key} moved focus behind the dropdown before a repaint"
            );
        })
        .unwrap();
        cx.run_until_parked();
        assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
    }
    overlay_key(cx, win, "enter");
    assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
    overlay_key(cx, win, "escape");
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.platform_menu_open.is_none() && app.menu_overlay.is_some()
        }),
        "first Escape must leave Settings drawn"
    );
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .settings_trap_contains_focus_for_e2e(window, cx)
        })
        .unwrap(),
        "the Settings trap resumes when the menu closes"
    );
    overlay_key(cx, win, "escape");
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    assert!(
        root_has_focus(cx, &app, win),
        "Settings returns to the original root"
    );
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_settings_focus");
}

/// #990 row 2: Settings yields when a modal arrives even with the dropdown
/// in front; two Escapes dismiss menu then modal, never an unseen Settings.
pub fn scenario_platform_menu_modal_settings_order(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "dirty before modal\n").unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount_short(cx, &repo);
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx));
        app.open_stash_push_modal(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            e2e::menu_is_front(app, cx)
                && app.stash_push_modal().is_some()
                && app.menu_overlay.is_none()
        }),
        "Settings must yield under the modal despite the dropdown above it"
    );
    overlay_key(cx, win, "enter");
    assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_some()));
    overlay_key(cx, win, "escape");
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.platform_menu_open.is_none()
                && app.stash_push_modal().is_some()
                && app.menu_overlay.is_none()
        }),
        "first Escape dismisses only the dropdown"
    );
    assert!(
        cx.update_window(win, |_, window, cx| {
            let input = app
                .read(cx)
                .stash_push_modal()
                .and_then(|modal| modal.input_state.as_ref())
                .expect("Stash modal input");
            input.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap(),
        "the Stash input, not the hidden Settings trap or root, resumes after the menu"
    );
    overlay_key(cx, win, "escape");
    assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_none()));
    assert!(root_has_focus(cx, &app, win));
    assert_eq!(repo_fingerprint(&repo), before, "no Git write");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_modal_settings_order");
}

/// #990 row 3: a real dropdown click closes the menu before command dispatch;
/// a harmless View toggle leaves Settings and a focus-changing one closes it.
pub fn scenario_platform_menu_settings_command_order(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount_short(cx, &repo);
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    let sidebar_before = cx.read(|cx| app.read(cx).sidebar.visible);
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    crate::app_conflict::click_control(cx, win, "platform-menu-cmd-view.toggleSidebar");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.platform_menu_open.is_none()
                && app.menu_overlay.is_some()
                && app.sidebar.visible != sidebar_before
        }),
        "sidebar toggle runs after dropdown close without dismissing Settings"
    );
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .settings_trap_contains_focus_for_e2e(window, cx)
        })
        .unwrap(),
        "Settings regains focus after a pure View toggle"
    );
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    let bottom_before = cx.read(|cx| app.read(cx).bottom_panel_open);
    crate::app_conflict::click_control(cx, win, "platform-menu-cmd-view.toggleTerminal");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.platform_menu_open.is_none()
                && app.menu_overlay.is_none()
                && app.bottom_panel_open != bottom_before
        }),
        "focus-changing toggle closes Settings along with the dropdown"
    );
    assert!(
        root_has_focus(cx, &app, win),
        "must not restore a hidden pane or trap"
    );
    // A focus-taking command is invoked by an actual dropdown click. Its
    // Settings trap must take the keyboard without a stale menu intercepting it.
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("Kagi", cx))
    });
    cx.run_until_parked();
    crate::app_conflict::click_control(cx, win, "platform-menu-cmd-app.settings");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).platform_menu_open.is_none())
            && cx
                .update_window(win, |_, window, cx| {
                    app.read(cx)
                        .settings_trap_contains_focus_for_e2e(window, cx)
                })
                .unwrap(),
        "dropdown must be gone and the new Settings trap must own focus"
    );
    overlay_key(cx, win, "escape");
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_settings_command_order");
}

/// #990 row 4: the dropdown and old Settings trap cannot survive a new tab or
/// a newly opened repository; the new screen takes focus without an old handle.
pub fn scenario_platform_menu_repository_navigation(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let fixture_c = build_fixture();
    let a = fixture_a.path().canonicalize().unwrap();
    let b = fixture_b.path().canonicalize().unwrap();
    let c = fixture_c.path().canonicalize().unwrap();
    let (app, win) = mount_short(cx, &a);
    app.update(cx, |app, cx| assert!(app.open_repository(b.clone(), cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    overlay_key(cx, win, "tab");
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.repo_path.as_ref() == Some(&b) && app.platform_menu_open.is_none()
        }),
        "tab switch must dismiss the old dropdown"
    );
    cx.update_window(win, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root");
        window.focus(&root, cx);
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| assert!(app.open_repository(c.clone(), cx)));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.repo_path.as_ref() == Some(&c)
                && app.platform_menu_open.is_none()
                && app.menu_overlay.is_none()
        }),
        "opening another repository must dismiss the old dropdown and trap"
    );
    assert!(
        root_has_focus(cx, &app, win),
        "new repository root owns focus"
    );
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_repository_navigation");
}

/// A real File > Close Tab click closes the old screen's Settings before the
/// new tab can open a dropdown of its own (#990 review, P2-2).
pub fn scenario_platform_menu_close_tab_command_navigation(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let a = fixture_a.path().canonicalize().unwrap();
    let b = fixture_b.path().canonicalize().unwrap();
    let (app, win) = mount_short(cx, &a);
    app.update(cx, |app, cx| assert!(app.open_repository(b.clone(), cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("File", cx))
    });
    cx.run_until_parked();
    crate::app_conflict::click_control(cx, win, "platform-menu-cmd-file.closeTab");
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.repo_path.as_ref() == Some(&b)
                && app.menu_overlay.is_none()
                && app.platform_menu_open.is_none()
        }),
        "command navigation must discard the departing Settings before another menu opens"
    );
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)),
        "B's dropdown must not self-close because A held the old pending focus"
    );
    assert!(root_has_focus(cx, &app, win));
    overlay_key(cx, win, "escape");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_close_tab_command_navigation");
}

/// The palette's existing search input resumes typing after the dropdown
/// releases keyboard ownership; focus must not remain on the window root.
pub fn scenario_platform_menu_palette_focus_return(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount_short(cx, &repo);
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.open_command_palette(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let input_focus = cx
        .update_window(win, |_, window, cx| {
            window.focused(cx).expect("palette input")
        })
        .unwrap();
    assert!(!root_has_focus(cx, &app, win));
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("View", cx))
    });
    cx.run_until_parked();
    assert!(
        root_has_focus(cx, &app, win),
        "dropdown takes the search input"
    );
    overlay_key(cx, win, "escape");
    assert!(
        cx.update_window(win, |_, window, _| input_focus.is_focused(window))
            .unwrap(),
        "closing the dropdown must return focus to the retained palette input"
    );
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_some()));
    overlay_key(cx, win, "escape");
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS platform_menu_palette_focus_return");
}

/// #1039: a menu snapshot belongs to the screen that produced it, not the
/// next repository's command target. Closing applies to the other menu
/// overlays too, without changing their behavior within one screen.
pub fn scenario_branch_picker_screen_departure(cx: &mut VisualTestAppContext) {
    let first = build_fixture();
    let second = build_fixture();
    let a = first.path().canonicalize().unwrap();
    let b = second.path().canonicalize().unwrap();
    git(&a, &["branch", "a-only"]);
    git(&b, &["branch", "b-only"]);
    let (app, win) = mount_short(cx, &a);
    app.update(cx, |app, cx| assert!(app.open_repository(b.clone(), cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    let invoke = |cx: &mut VisualTestAppContext, id| {
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| app.handle_menu_command(id, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
    };
    invoke(cx, "branch.checkout");
    assert!(
        cx.read(|cx| matches!(
            &app.read(cx).menu_overlay,
            Some(MenuOverlay::BranchPicker { mode: BranchPickerMode::Checkout, branches })
                if branches.iter().any(|name| name == "a-only")
                    && !branches.iter().any(|name| name == "b-only")
        )),
        "A's picker snapshots A's local branches"
    );
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "A's BranchPicker must close before B can use a stale branch name"
    );
    cx.run_until_parked();
    invoke(cx, "branch.checkout");
    assert!(
        cx.read(|cx| matches!(
            &app.read(cx).menu_overlay,
            Some(MenuOverlay::BranchPicker { branches, .. })
                if branches.iter().any(|name| name == "b-only")
                    && !branches.iter().any(|name| name == "a-only")
        )),
        "B's new picker must contain only B's branches"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    cx.run_until_parked();

    invoke(cx, "app.about");
    assert!(cx.read(|cx| matches!(&app.read(cx).menu_overlay, Some(MenuOverlay::Info { .. }))));
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "Info closes on departure"
    );
    cx.run_until_parked();
    invoke(cx, "view.commandPalette");
    assert!(cx.read(|cx| matches!(
        &app.read(cx).menu_overlay,
        Some(MenuOverlay::CommandPalette)
    )));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_none()),
        "Command Palette closes on departure"
    );
    cx.run_until_parked();
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS branch_picker_screen_departure");
}
