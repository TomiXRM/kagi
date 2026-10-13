//! Terminal lifecycle through the real isolated PTY, not the user's shell.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang, Msg},
    terminal::{self, ShellExit, TerminalState},
    theme, BottomTab, KagiApp,
};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    terminal::take_rendered_state_for_e2e();
    for name in [
        "terminal-restart",
        "terminal-state-title",
        "terminal-state-hint",
        "terminal-state-icon",
    ] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn assert_block(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    state: TerminalState,
    title: &str,
) {
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "terminal-restart").is_some(),
        "Exited(3) must paint a Restart button"
    );
    crate::recovery_unborn_empty::assert_centered_block(
        window,
        "terminal-empty-state",
        [
            "terminal-state-icon",
            "terminal-state-title",
            "terminal-state-hint",
            "terminal-restart",
        ],
    );
    let rendered = terminal::take_rendered_state_for_e2e().expect("empty state rendered");
    assert_eq!(rendered.state, state);
    assert_eq!(rendered.title.as_ref(), title);
    assert_eq!(rendered.restart_role, gpui::Role::Button);
    assert_eq!(rendered.restart_name.as_ref(), Msg::TerminalRestart.t());
    assert!(!rendered.hint.is_empty());
    if let TerminalState::Failed { reason } = state {
        assert!(
            rendered.hint.contains(&reason),
            "failed hint must retain the OS reason"
        );
        assert!(!reason.is_empty());
    }
    let panel = e2e::control_bounds(window.window_id(), "bottom-panel").unwrap();
    for name in [
        "terminal-restart",
        "terminal-state-title",
        "terminal-state-hint",
        "terminal-state-icon",
    ] {
        let rect = e2e::control_bounds(window.window_id(), name).unwrap();
        assert!(rect.size.width > gpui::px(0.) && rect.size.height > gpui::px(0.));
        assert!(
            rect.left() >= panel.left()
                && rect.top() >= panel.top()
                && rect.right() <= panel.right() + gpui::px(1.)
                && rect.bottom() <= panel.bottom() + gpui::px(1.),
            "{name} outside panel: {rect:?} {panel:?}"
        );
    }
}

fn assert_running(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.read(|cx| {
        let session = app.read(cx).ui().terminal_session.as_ref().unwrap();
        assert_eq!(session.state, TerminalState::Running);
        assert!(session.view.is_some() && session.paste_writer.is_some());
    });
    draw(cx, window);
    assert!(terminal::take_rendered_state_for_e2e().is_none());
    assert!(e2e::control_bounds(window.window_id(), "terminal-restart").is_none());
}

fn exit_three(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .write_input("exit 3\n");
    });
    wait_for_report(cx, app, ShellExit::Exited { code: 3 });
    let lines = kagi_ui_core::klog::tail();
    let started = lines
        .iter()
        .rposition(|line| line.starts_with("[kagi] terminal: started shell="))
        .expect("this shell's start remains in the bounded log tail");
    let lifetime = &lines[started + 1..];
    assert_eq!(
        lifetime
            .iter()
            .filter(|line| line.as_str() == "[kagi] terminal: shell exited")
            .count(),
        1,
        "known shell exit emits its contract line exactly once"
    );
    let exit_index = lifetime
        .iter()
        .position(|line| line == "[kagi] terminal: shell exited")
        .unwrap();
    let wait_index = lifetime
        .iter()
        .position(|line| line.starts_with("[kagi] terminal: shell wait:"))
        .unwrap();
    assert!(
        exit_index < wait_index,
        "exit contract line precedes wait-status line"
    );
}

fn press_restart(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    cx.update_window(window, |_, window, cx| {
        let focus = terminal::take_restart_focus_for_e2e();
        assert!(
            (0..100).any(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                focus.is_focused(window)
            }),
            "Restart must be reachable by Tab traversal"
        );
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, key);
}

fn show_terminal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::Terminal;
        cx.notify();
    });
    draw(cx, window);
}

fn exited_restarts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    let title = match lang {
        Lang::En => "Shell exited (code 3)",
        Lang::Ja => "shell が終了しました（終了コード 3）",
    };
    for key in ["enter", "space"] {
        exit_three(cx, app);
        assert_block(cx, window, TerminalState::Exited { code: 3 }, title);
        let spawns = cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns);
        crate::app_conflict::click_control(cx, window, "tab-oplog");
        crate::app_conflict::click_control(cx, window, "tab-terminal");
        assert_block(cx, window, TerminalState::Exited { code: 3 }, title);
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns),
            spawns,
            "returning to Exited must not respawn until Restart"
        );
        press_restart(cx, window, key);
        assert_running(cx, app, window);
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().terminal_session.as_ref().unwrap().spawns),
            spawns + 1
        );
    }
}

fn failed_recovers(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    lang: Lang,
    repo: &std::path::Path,
) {
    exit_three(cx, app);
    draw(cx, window);
    KagiApp::set_terminal_shell_for_e2e(Some(repo.join("nonexistent-shell").display().to_string()));
    crate::app_conflict::click_control(cx, window, "terminal-restart");
    // Failed starts still auto-open Operation Log. The real Terminal tab must
    // return to Failed without immediately retrying or bouncing back to the log.
    let failed_attempts = || {
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.starts_with("[kagi] terminal: start failed:"))
            .count()
    };
    let attempts = failed_attempts();
    crate::app_conflict::click_control(cx, window, "tab-terminal");
    assert_eq!(cx.read(|cx| app.read(cx).bottom_tab), BottomTab::Terminal);
    assert_eq!(failed_attempts(), attempts, "showing Failed must not retry");
    let failed = cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .state
            .clone()
    });
    assert!(
        matches!(&failed, TerminalState::Failed { reason } if reason.contains("nonexistent-shell"))
    );
    assert_block(
        cx,
        window,
        failed,
        match lang {
            Lang::En => "Shell failed",
            Lang::Ja => "shell でエラーが発生しました",
        },
    );
    let _retry_shell = crate::gui_isolation::StandInShell::install();
    press_restart(cx, window, "space");
    assert_running(cx, app, window);
}

fn wait_for_report(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, expected: ShellExit) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .unwrap()
                .shell
                .as_ref()
                .unwrap()
                .exit
                .as_ref()
                == Some(&expected)
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "shell wait did not report {expected:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn unknown_wait_recovers(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    // Use the scenario's shell guard, which outlives the restarted child.
    exit_three(cx, app);
    draw(cx, window);
    let reason = "wait: Interrupted system call (os error 4)";
    KagiApp::set_next_terminal_wait_error_for_e2e(Some(reason.into()));
    press_restart(cx, window, "enter");
    assert_running(cx, app, window);
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .write_input("exit 3\n")
    });
    wait_for_report(cx, app, ShellExit::Unknown(reason.into()));
    assert_block(
        cx,
        window,
        TerminalState::Failed {
            reason: reason.into(),
        },
        match lang {
            Lang::En => "Shell failed",
            Lang::Ja => "shell でエラーが発生しました",
        },
    );
    cx.read(|cx| {
        let state = app.read(cx);
        let session = state.ui().terminal_session.as_ref().unwrap();
        assert!(session.view.is_none() && session.paste_writer.is_none());
        assert!(!session.release_offer_pending);
        assert!(state.unlock_worktree_modal().is_none());
    });
    let lines = kagi_ui_core::klog::tail();
    let started = lines
        .iter()
        .rposition(|line| line.starts_with("[kagi] terminal: started shell="))
        .unwrap();
    let lifetime = &lines[started + 1..];
    assert!(
        !lifetime
            .iter()
            .any(|line| line == "[kagi] terminal: shell exited"),
        "Unknown must not claim a proven shell exit"
    );
    assert_eq!(
        lifetime
            .iter()
            .filter(|line| line.starts_with("[kagi] terminal: shell wait:")
                && line.ends_with("status=unknown"))
            .count(),
        1
    );
    press_restart(cx, window, "space");
    assert_running(cx, app, window);
}

pub fn scenario_terminal_states(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _ports = crate::gui_isolation::PortStore::keep();
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            i18n::set_lang(lang);
            theme::set_zoom(zoom);
            cx.update(theme::sync_gpui_component_theme);
            let _shell = crate::gui_isolation::StandInShell::install();
            let (app, window) = mount(cx, fixture.path());
            show_terminal(cx, &app, window);
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.ensure_terminal(window, cx);
                    cx.notify();
                });
            })
            .unwrap();
            assert_running(cx, &app, window);
            exited_restarts(cx, &app, window, lang);
            unknown_wait_recovers(cx, &app, window, lang);
            failed_recovers(cx, &app, window, lang, fixture.path());
            unmount(cx, app, window);

            // failed_recovers' scoped shell resets the seam on drop.
            let _shell = crate::gui_isolation::StandInShell::install();
            let (app, window) = mount(cx, fixture.path());
            show_terminal(cx, &app, window);
            assert_block(
                cx,
                window,
                TerminalState::NotStarted,
                match lang {
                    Lang::En => "Shell not started",
                    Lang::Ja => "shell は未起動です",
                },
            );
            crate::app_conflict::click_control(cx, window, "terminal-restart");
            assert_running(cx, &app, window);
            unmount(cx, app, window);
        }
    }
    assert_eq!(before, repo_fingerprint(fixture.path()));
}
