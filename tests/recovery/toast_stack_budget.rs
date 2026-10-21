//! #1551 / #1590: bounded notifications stay bottom-left without blocking work.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{px, AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme, KagiApp};
use std::time::{Duration, Instant};

pub fn scenario_toast_stack_budget(cx: &mut VisualTestAppContext) {
    repeated_noop_push(cx);
    operation_result_details(cx);
    run_budget_matrix(cx, 1.);
}

pub fn scenario_toast_stack_budget_large_zoom(cx: &mut VisualTestAppContext) {
    run_budget_matrix(cx, 1.67);
}

pub fn scenario_toast_stack_budget_compact_editor(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    // At 167%, the Editor's reserved 200 logical pixels clip the preferred
    // compact panel body. The taller 720px matrix does not reliably hit that cap.
    let dimensions = (940., 600.);
    for preset in ["apple-light", "apple-dark"] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            let fixture = build_fixture();
            let repo = fixture.path();
            git(
                repo,
                &[
                    "remote",
                    "add",
                    "origin",
                    repo.join("missing").to_str().unwrap(),
                ],
            );
            let (app, window) = mount(cx, repo);
            app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
            crate::pr_detail_adaptive::resize(cx, window, dimensions);
            theme::set_active(preset);
            theme::set_zoom(1.67);
            app.update(cx, |app, cx| {
                app.set_lang(lang, cx);
                app.bottom_tab = kagi::ui::BottomTab::Activity;
                cx.notify();
            });
            for attempt in 0..4 {
                git(
                    repo,
                    &[
                        "remote",
                        "set-url",
                        "origin",
                        repo.join(format!("missing-{attempt}")).to_str().unwrap(),
                    ],
                );
                app.update(cx, |app, cx| app.fetch_async(false, cx));
                let deadline = Instant::now() + Duration::from_secs(30);
                while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
                    cx.run_until_parked();
                    assert!(Instant::now() < deadline, "failed fetch settles");
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            // Repeated workflow results replace one key. Fill the remaining
            // slots with independent layout fixtures to exercise the cap.
            app.update(cx, |app, cx| {
                let stack = app.toast_stack.clone().unwrap();
                stack.update(cx, |stack, cx| {
                    assert_eq!(stack.toasts().len(), 1);
                    for n in 0..3 {
                        stack.push_notify(
                            kagi::ui::ToastKind::Error,
                            format!("Layout fixture {n}"),
                            cx,
                        );
                    }
                });
                app.bottom_panel_open = true;
                cx.notify();
            });
            crate::native_ax::activate(cx);
            draw(cx, window);
            assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
            let graph_tabs = e2e::control_bounds(window.window_id(), "tab-oplog")
                .expect("compact Graph primary tabs painted");
            app.update(cx, |app, cx| app.open_editor_workspace(cx));
            cx.run_until_parked();
            draw(cx, window);
            assert!(
                cx.read(|cx| app.read(cx).ui().editor_workspace.is_some()),
                "real Editor workspace is active"
            );
            let editor_tabs = e2e::control_bounds(window.window_id(), "tab-oplog")
                .expect("compact Editor primary tabs painted");
            assert!(
                editor_tabs.bottom() > graph_tabs.bottom() + px(10.),
                "exercise editor_cap clipping, not only the generic panel limit: \
                 Graph {graph_tabs:?}, Editor {editor_tabs:?}"
            );
            let collapsed = assert_bottom_left(cx, window, "toast-stack", dimensions, true);
            let more = i18n::Msg::ToastMore.t().replace("{count}", "3");
            assert!(crate::native_ax::press(cx, window, &more));
            cx.run_until_parked();
            let expanded = assert_bottom_left(cx, window, "toast-stack", dimensions, true);
            assert!(
                expanded.top() <= collapsed.top(),
                "disclosure expands upward within its cap"
            );
            assert_eq!(
                crate::native_ax::node(cx, window, ("toast-summary", 0u64).into())["aria"]
                    ["expanded"],
                true
            );
            assert!(
                (expanded.bottom() - collapsed.bottom()).abs() <= px(1.),
                "Editor expansion retains the status-bar anchor"
            );
            assert!(
                expanded.top() >= editor_tabs.bottom(),
                "expanded toast must not cover compact Editor primary bottom tabs: \
                 stack {expanded:?}, tabs {editor_tabs:?}, {preset}/{lang:?}/167%"
            );
            cx.simulate_mouse_move(window, editor_tabs.center(), None, gpui::Modifiers::none());
            cx.simulate_click(window, editor_tabs.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            assert_eq!(
                cx.read(|cx| app.read(cx).bottom_tab),
                kagi::ui::BottomTab::OperationLog,
                "compact Editor primary tabs remain hit-testable with expanded notifications"
            );
            unmount(cx, app, window);
        }
    }
    theme::set_zoom(old_zoom);
    theme::set_active(&old_theme);
    i18n::set_lang(old_lang);
    eprintln!("[gui-e2e] PASS toast_stack_budget_compact_editor");
}

fn run_budget_matrix(cx: &mut VisualTestAppContext, zoom: f32) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "lang",
        "ui_zoom",
        "theme",
        "settings_pane",
        "settings_window_frame",
        "settings_window_position",
        "settings_window_height",
    ]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    for dimensions in [(940., 720.), (1000., 720.), (1400., 900.)] {
        for preset in ["apple-light", "apple-dark"] {
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                for distinct in [false, true] {
                    let fixture = build_fixture();
                    let repo = fixture.path();
                    git(
                        repo,
                        &[
                            "remote",
                            "add",
                            "origin",
                            repo.join("missing").to_str().unwrap(),
                        ],
                    );
                    let (app, window) = mount(cx, repo);
                    crate::pr_detail_adaptive::resize(cx, window, dimensions);
                    theme::set_active(preset);
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| app.set_lang(lang, cx));
                    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
                    let before = cx.read(|cx| {
                        app.read(cx)
                            .op_log
                            .as_ref()
                            .unwrap()
                            .read(cx)
                            .entries()
                            .len()
                    });
                    for attempt in 0..4 {
                        if distinct {
                            git(
                                repo,
                                &[
                                    "remote",
                                    "set-url",
                                    "origin",
                                    repo.join(format!("missing-{attempt}")).to_str().unwrap(),
                                ],
                            );
                        }
                        app.update(cx, |app, cx| app.fetch_async(false, cx));
                        let deadline = Instant::now() + Duration::from_secs(30);
                        while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
                            cx.run_until_parked();
                            assert!(Instant::now() < deadline, "failed fetch settles");
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    }
                    app.update(cx, |app, cx| {
                        let stack = app.toast_stack.clone().unwrap();
                        stack.update(cx, |stack, cx| {
                            assert_eq!(
                                stack.toasts().len(),
                                1,
                                "same repo/request/outcome replaces even when raw errors differ"
                            );
                            for n in 0..3 {
                                stack.push_notify(
                                    kagi::ui::ToastKind::Error,
                                    format!("Layout fixture {n}"),
                                    cx,
                                );
                            }
                        });
                        app.bottom_panel_open = true;
                        cx.notify();
                    });
                    crate::native_ax::activate(cx);
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let stack = e2e::control_bounds(window.window_id(), "toast-stack")
                        .expect("stack painted");
                    assert!(
                        stack.size.height <= px(dimensions.1 * 0.3 + 1.),
                        "toast stack height {:?} exceeds viewport budget",
                        stack.size.height
                    );
                    assert_bottom_left(cx, window, "toast-stack", dimensions, true);
                    assert!(
                        (stack.bottom() - (px(dimensions.1) - theme::scaled_px(34.))).abs()
                            <= px(2.),
                        "repository toast stack stays exactly above the status-bar reserve"
                    );
                    let more = i18n::Msg::ToastMore.t().replace("{count}", "3");
                    assert!(
                        crate::native_ax::press(cx, window, &more),
                        "named native disclosure"
                    );
                    cx.run_until_parked();
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    assert!(crate::native_ax::press(
                        cx,
                        window,
                        i18n::Msg::ToastCollapse.t()
                    ));
                    cx.run_until_parked();
                    draw(cx, window);
                    let mut reached = false;
                    for _ in 0..120 {
                        crate::keyboard_nav::keys(cx, window, "tab");
                        cx.update_window(window, |_, window, cx| {
                            window.refresh();
                            window.draw(cx).clear();
                        })
                        .unwrap();
                        let tree = crate::native_ax::tree(cx, window);
                        if tree["gpui_focus"]
                            .as_str()
                            .is_some_and(|id| tree["nodes"][id]["aria"]["label"] == more)
                        {
                            reached = true;
                            break;
                        }
                    }
                    assert!(reached, "Tab reaches named disclosure");
                    crate::keyboard_nav::keys(cx, window, "enter");
                    cx.run_until_parked();
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let disclosure =
                        crate::native_ax::node(cx, window, ("toast-summary", 0u64).into());
                    assert_eq!(disclosure["aria"]["expanded"], true);
                    let expanded = e2e::control_bounds(window.window_id(), "toast-stack").unwrap();
                    assert!(expanded.size.height <= px(dimensions.1 * 0.3 + 1.));
                    assert!(expanded.top() <= stack.top(), "expansion grows upward");
                    assert!(
                        (expanded.bottom() - stack.bottom()).abs() <= px(1.),
                        "expansion keeps its bottom anchor"
                    );
                    assert_bottom_left(cx, window, "toast-stack", dimensions, true);
                    if kagi_domain::compact::is_compact(
                        dimensions.0,
                        theme::rem_size_px(),
                        theme::compact_threshold_rem(),
                    ) {
                        let tabs = e2e::control_bounds(window.window_id(), "tab-oplog").unwrap();
                        assert!(
                            expanded.top() >= tabs.bottom(),
                            "compact tabs exposed: stack {expanded:?}, tabs {tabs:?}"
                        );
                    }
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert_eq!(
                            state.toast_stack.as_ref().unwrap().read(cx).toasts().len(),
                            4
                        );
                        assert_eq!(
                            state.op_log.as_ref().unwrap().read(cx).entries().len(),
                            before + 4,
                            "all failed fetch receipts retained"
                        );
                    });
                    let row = e2e::control_bounds(window.window_id(), "graph-row-1")
                        .expect("Graph row painted");
                    let hit = gpui::point(row.left() + px(40.), row.center().y);
                    cx.simulate_mouse_move(window, hit, None, gpui::Modifiers::none());
                    cx.simulate_click(window, hit, gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert_eq!(
                        cx.read(|cx| app.read(cx).ui().selected),
                        Some(1),
                        "Graph remains hit-testable"
                    );
                    let terminal = e2e::control_bounds(window.window_id(), "tb-terminal")
                        .expect("Terminal toolbar control painted");
                    cx.simulate_mouse_move(
                        window,
                        terminal.center(),
                        None,
                        gpui::Modifiers::none(),
                    );
                    cx.simulate_click(window, terminal.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert_eq!(
                        cx.read(|cx| app.read(cx).bottom_tab),
                        kagi::ui::BottomTab::Terminal,
                        "toolbar remains hit-testable"
                    );
                    let entry = if kagi_domain::compact::is_compact(
                        dimensions.0,
                        theme::rem_size_px(),
                        theme::compact_threshold_rem(),
                    ) {
                        "tab-oplog"
                    } else {
                        "status-icon-oplog"
                    };
                    let tab = e2e::control_bounds(window.window_id(), entry)
                        .expect("Operation Log entry");
                    cx.simulate_click(window, tab.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    assert!(
                        crate::native_ax::press(cx, window, i18n::Msg::ToastCollapse.t()),
                        "expanded disclosure remains available: {}",
                        crate::native_ax::tree(cx, window)
                    );
                    assert_eq!(
                        cx.read(|cx| app.read(cx).bottom_tab),
                        kagi::ui::BottomTab::OperationLog,
                        "Operation Log entry remains hit-testable while notifications are expanded"
                    );
                    cx.run_until_parked();
                    draw(cx, window);
                    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
                    let summary = cx
                        .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(0))
                        .expect("receipt row painted");
                    cx.simulate_click(
                        window,
                        gpui::point(summary.center().x, summary.top() + px(8.)),
                        gpui::Modifiers::none(),
                    );
                    cx.run_until_parked();
                    assert_eq!(
                        cx.read(|cx| panel.read(cx).expanded()),
                        Some(0),
                        "receipt details remain hit-testable"
                    );
                    drop(panel);
                    // The production native Settings host shares the retained stack.
                    cx.simulate_keystrokes(window, "cmd-,");
                    cx.run_until_parked();
                    let settings = cx
                        .read(|cx| cx.windows())
                        .into_iter()
                        .find(|other| other.window_id() != window.window_id())
                        .expect("separate native Settings window");
                    let settings_dimensions = cx
                        .update_window(settings, |_, window, _| {
                            let size = window.viewport_size();
                            (f32::from(size.width), f32::from(size.height))
                        })
                        .unwrap();
                    assert!(
                        (settings_dimensions.0 - 715.).abs() <= 1.,
                        "Settings uses its production fixed width"
                    );
                    crate::native_ax::activate(cx);
                    draw(cx, settings);
                    let settings_stack =
                        assert_bottom_left(cx, settings, "toast-stack", settings_dimensions, false);
                    assert!(
                        (settings_stack.bottom()
                            - (px(settings_dimensions.1) - theme::scaled_px(34.)))
                        .abs()
                            <= px(2.),
                        "native Settings retains the same fixed bottom anchor"
                    );
                    assert!(crate::native_ax::press(cx, settings, &more),
                            "Settings disclosure {dimensions:?}/{zoom}/{preset}/{lang:?}/{distinct}: {}",
                            crate::native_ax::tree(cx, settings));
                    cx.run_until_parked();
                    draw(cx, settings);
                    let settings_expanded =
                        assert_bottom_left(cx, settings, "toast-stack", settings_dimensions, false);
                    assert!(settings_expanded.top() <= settings_stack.top());
                    assert!(
                        (settings_expanded.bottom() - settings_stack.bottom()).abs() <= px(1.),
                        "Settings expansion grows upward from a fixed bottom anchor"
                    );
                    assert!(crate::native_ax::press(
                        cx,
                        settings,
                        i18n::Msg::ToastCollapse.t()
                    ));
                    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
                    KagiApp::hold_next_fetch_for_e2e(hold);
                    app.update(cx, |app, cx| app.fetch_async(false, cx));
                    cx.run_until_parked();
                    for (host, repository, host_dimensions) in [
                        (window, true, dimensions),
                        (settings, false, settings_dimensions),
                    ] {
                        draw(cx, host);
                        assert_bottom_left(cx, host, "busy-snackbar", host_dimensions, repository);
                        assert_bottom_left(cx, host, "toast-stack", host_dimensions, repository);
                    }
                    release.send(());
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
                        cx.run_until_parked();
                        assert!(Instant::now() < deadline, "held fetch settles");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    cx.update_window(settings, |_, window, _| window.remove_window())
                        .unwrap();
                    cx.run_until_parked();
                    unmount(cx, app, window);
                }
            }
        }
    }
    theme::set_zoom(old_zoom);
    theme::set_active(&old_theme);
    i18n::set_lang(old_lang);
    eprintln!("[gui-e2e] PASS toast_stack_budget");
}

fn repeated_noop_push(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    let repo = fixture.path();
    git(
        repo,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(repo, &["push", "-qu", "origin", "HEAD"]);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let old_lang = i18n::lang();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            let mut result_key = None;
            for _ in 0..3 {
                let before = kagi_git::read_oplog_tail_for_repo(repo, 100).len();
                let requested_at = Instant::now();
                app.update(cx, |app, cx| {
                    app.status_footer = kagi::ui::FooterStatus::Idle("".into());
                    app.open_push_modal(cx);
                });
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    cx.run_until_parked();
                    let presented = cx.read(|cx| {
                        let state = app.read(cx);
                        let expected = i18n::Msg::AlreadyUpToDatePush.t();
                        if active {
                            matches!(&state.status_footer,
                                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == expected)
                        } else {
                            state
                                .toast_stack
                                .as_ref()
                                .unwrap()
                                .read(cx)
                                .toasts()
                                .iter()
                                .any(|toast| {
                                    toast.message.as_ref() == expected && toast.born >= requested_at
                                })
                        }
                    });
                    if presented {
                        break;
                    }
                    assert!(Instant::now() < deadline, "no-op Push result settles");
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert_eq!(
                    kagi_git::read_oplog_tail_for_repo(repo, 100).len(),
                    before,
                    "a no-op Push request creates no write receipt"
                );
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(
                        state.push_modal().is_none(),
                        "no-op Push needs no confirmation"
                    );
                    assert!(e2e::app_notice_message(state).is_none());
                    assert!(
                        !state.bottom_panel_open,
                        "Push must not auto-open Operation Log"
                    );
                    let stack = state.toast_stack.as_ref().unwrap().read(cx);
                    assert_eq!(
                        stack.toasts().len(),
                        usize::from(!active),
                        "current quick Push is footer-only; inactive repeats replace"
                    );
                    if !active {
                        let toast = &stack.toasts()[0];
                        assert_eq!(toast.kind, kagi::ui::ToastKind::Sync);
                        let key = toast.result_key.as_ref().expect("no-op Push result key");
                        if let Some(previous) = &result_key {
                            assert_eq!(key, previous, "repeated Push retains the same result key");
                        }
                        result_key = Some(key.clone());
                    }
                });
            }
        }
        unmount(cx, app, window);
    }
    i18n::set_lang(old_lang);
}

fn operation_result_details(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let old_lang = i18n::lang();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for destination in ["home", "other-tab", "closed-owner", "removed-owner"] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            git(
                &repo,
                &[
                    "remote",
                    "add",
                    "origin",
                    repo.join("missing-remote").to_str().unwrap(),
                ],
            );
            let other = (destination == "other-tab").then(build_fixture);
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(lang, cx);
                app.set_window_active_for_e2e(true, cx);
                app.bottom_panel_open = false;
                app.bottom_tab = kagi::ui::BottomTab::Terminal;
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.fetch_async(false, cx);
                    if let Some(other) = &other {
                        assert!(app.open_repository(other.path().to_path_buf(), cx));
                    } else {
                        app.open_home_tab(window, cx);
                    }
                });
            })
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                cx.run_until_parked();
                let settled = cx.read(|cx| {
                    let state = app.read(cx);
                    state.fetch_in_flight.is_none()
                        && state
                            .toast_stack
                            .as_ref()
                            .unwrap()
                            .read(cx)
                            .toasts()
                            .iter()
                            .any(|toast| {
                                toast.kind == kagi::ui::ToastKind::Error && toast.action.is_some()
                            })
                });
                if settled {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "background failure presents its detail action"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            let recorded = kagi_git::read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .find(|entry| entry.op == "fetch")
                .expect("real failed Fetch is durable");
            assert!(recorded.id > 0);
            assert!(matches!(
                recorded.outcome,
                kagi_git::oplog::OpOutcome::Failed { .. }
            ));
            if matches!(destination, "closed-owner" | "removed-owner") {
                app.update(cx, |app, cx| app.close_tab(0, cx));
                if destination == "removed-owner" {
                    std::fs::remove_dir_all(&repo).unwrap();
                }
            }
            cx.read(|cx| {
                assert!(
                    !app.read(cx).bottom_panel_open,
                    "failure never auto-opens details"
                )
            });
            draw(cx, window);
            let action = e2e::control_bounds(window.window_id(), "operation-result-details")
                .expect("failure detail action is actually drawn on this screen");
            if lang == i18n::Lang::Ja {
                assert!(crate::native_ax::press(
                    cx,
                    window,
                    i18n::Msg::BottomTabOperationLog.t()
                ));
            } else {
                cx.simulate_click(window, action.center(), gpui::Modifiers::none());
            }
            cx.run_until_parked();
            draw(cx, window);
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.bottom_panel_open);
                assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                if matches!(destination, "home" | "other-tab") {
                    assert!(
                        !state.home_in_front(),
                        "detail action enters the operation owner"
                    );
                    assert_eq!(state.repo_path.as_deref(), Some(repo.as_path()));
                } else {
                    assert!(
                        state.home_in_front(),
                        "closed owner still has a visible host log"
                    );
                }
                let panel = state.op_log.as_ref().unwrap().read(cx);
                let expanded = panel
                    .expanded()
                    .expect("detail action expands its own receipt");
                assert_eq!(panel.entries()[expanded].id, recorded.id);
                assert!(
                    panel.scroll_handle().bounds_for_item(expanded).is_some(),
                    "the selected receipt is painted, not hidden behind Home"
                );
                let stack = state.toast_stack.as_ref().unwrap().read(cx);
                assert!(
                    stack
                        .toasts()
                        .iter()
                        .all(|toast| toast.dismissing.is_some()),
                    "only successful navigation dismisses the result"
                );
            });
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(old_lang);
    eprintln!("[gui-e2e] PASS operation result details from Home/other/closed/removed owner EN/JA");
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn assert_bottom_left(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    control: &str,
    dimensions: (f32, f32),
    repository: bool,
) -> Bounds<Pixels> {
    draw(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), control)
        .unwrap_or_else(|| panic!("{control} painted in native host"));
    // measure_inside records the busy card's content box inside its 1px border.
    let border = if control == "busy-snackbar" {
        px(1.)
    } else {
        px(0.)
    };
    assert!(
        (bounds.left() - theme::scaled_px(12.) - border).abs() <= px(1.),
        "{control} stays at the left inset: {bounds:?}"
    );
    assert!(bounds.right() <= px(dimensions.0));
    assert!(
        bounds.size.width <= theme::scaled_px(460.).min(px(dimensions.0 * 0.45)) + px(1.),
        "{control} retains its width cap"
    );
    assert!(
        bounds.size.height <= px(dimensions.1 * 0.3 + 1.),
        "{control} retains the viewport budget"
    );
    let bottom = px(dimensions.1) - theme::scaled_px(34.);
    assert!(
        bounds.bottom() <= bottom + px(1.),
        "{control} remains above the status-bar reserve: {bounds:?}"
    );
    assert!(
        bounds.bottom() > px(dimensions.1 * 0.5),
        "{control} is bottom-anchored, not at the toolbar: {bounds:?}"
    );
    if repository {
        let footer = e2e::control_bounds(window.window_id(), "pr-menu-footer")
            .expect("repository status bar painted");
        assert!(
            bounds.bottom() <= footer.top(),
            "{control} never covers status-bar controls"
        );
        if kagi_domain::compact::is_compact(
            dimensions.0,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        ) {
            for id in ["tb-terminal", "status-icon-oplog"] {
                let control = e2e::control_bounds(window.window_id(), id)
                    .unwrap_or_else(|| panic!("compact control {id} painted"));
                assert!(
                    bounds.right() <= control.left()
                        || bounds.left() >= control.right()
                        || bounds.bottom() <= control.top()
                        || bounds.top() >= control.bottom(),
                    "compact control {id} remains unobscured"
                );
            }
        }
    }
    bounds
}
