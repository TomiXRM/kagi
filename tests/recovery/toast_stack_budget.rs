//! One bounded bottom-left notification stack in every surface's centre content.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{px, AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::toast_stack::{ToastAudience, ToastSubject};
use kagi::ui::{e2e, i18n, theme, KagiApp};
use std::time::{Duration, Instant};

#[path = "toast_conflict_lane.rs"]
pub(crate) mod conflict_lane;

pub fn scenario_toast_stack_budget(cx: &mut VisualTestAppContext) {
    non_repository_lane_policy(cx);
    takeover_lane_policy(cx);
    portable_settings_scroll(cx);
    conflict_content_anchor(cx);
    conflict_lane::run(cx);
    repeated_noop_push(cx);
    operation_result_details(cx);
    run_budget_matrix(cx, 1.);
}

fn portable_settings_scroll(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
    theme::set_zoom(1.67);
    app.update(cx, |app, cx| app.set_lang(i18n::Lang::Ja, cx));
    cx.simulate_keystrokes(window, "cmd-,");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.windows().len()),
        1,
        "portable Settings shares its repository window"
    );
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_some()));
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let oldest = stack.update(cx, |stack, cx| {
        let mut oldest = 0;
        for index in 0..3 {
            let id = stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    ToastSubject::Settings,
                    kagi::ui::ToastKind::Error,
                    format!("Settings error {index}\nDurable second line\nDurable third line"),
                ),
                cx,
            );
            if index == 0 {
                oldest = id;
            }
        }
        oldest
    });
    crate::native_ax::activate(cx);
    draw(cx, window);
    assert!(crate::native_ax::press(
        cx,
        window,
        &i18n::Msg::ToastMore.t().replace("{count}", "2"),
    ));
    cx.run_until_parked();
    draw(cx, window);
    let viewport = e2e::control_bounds(window.window_id(), "toast-container").unwrap();
    let frame = e2e::control_bounds(window.window_id(), "settings-frame").unwrap();
    let detail = crate::settings_controls::bounds(cx, window, "settings-detail-scroll")
        .expect("actual fixed Settings detail viewport");
    for pane in kagi_ui_settings::Pane::ALL {
        let control =
            crate::settings_controls::bounds(cx, window, &format!("settings-pane-{}", pane.id()))
                .expect("actual sidebar navigation control");
        assert!(
            control.size.width > px(0.)
                && control.size.height > px(0.)
                && control.left() >= frame.left()
                && control.right() <= frame.right()
                && control.top() >= frame.top()
                && control.bottom() <= frame.bottom(),
            "{pane:?} control {control:?} remains inside Settings frame {frame:?}; detail {detail:?}"
        );
    }
    assert!(
        viewport.left() >= frame.left() && viewport.right() <= frame.right(),
        "toast {viewport:?}, Settings frame {frame:?}, fixed detail {detail:?}"
    );
    assert!(
        viewport.top() >= frame.top() && viewport.bottom() <= frame.bottom(),
        "toast {viewport:?}, Settings frame {frame:?}, fixed detail {detail:?}"
    );
    assert!(viewport.size.height <= px(660. * 0.3 + 1.));
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-10_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let close = format!("toast-dismiss-{oldest}");
    // Every redraw also prepaints the underlying empty repository host. It
    // must not clamp the Settings offset back to zero in the same window.
    for _ in 0..3 {
        e2e::clear_control_bounds(window.window_id(), &close);
        draw(cx, window);
        let control = e2e::control_bounds(window.window_id(), &close).unwrap();
        assert!(
            viewport.contains(&control.center()),
            "Settings oldest card stays reachable across repository prepaint"
        );
    }
    stack.update(cx, |stack, cx| {
        stack.push_keyed_notify(
            kagi::ui::toast_stack::ToastSpec::new(
                ToastSubject::Repository,
                kagi::ui::ToastKind::Error,
                "Independent repository feedback",
            ),
            cx,
        );
    });
    e2e::clear_control_bounds(window.window_id(), &close);
    draw(cx, window);
    let control = e2e::control_bounds(window.window_id(), &close).unwrap();
    assert!(
        viewport.contains(&control.center()),
        "foreign arrival does not reset Settings scroll"
    );
    let close_settings = e2e::control_bounds(window.window_id(), "settings-close").unwrap();
    cx.simulate_click(window, close_settings.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    e2e::clear_control_bounds(window.window_id(), &close);
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), &close).is_none());
    drop(stack);
    unmount(cx, app, window);
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
            // Geometry fixture: reserve the real external-change banner as
            // well as the workspace header, without executing a save/reload.
            let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().unwrap());
            editor.update(cx, |view, cx| {
                view.external_changed = true;
                cx.notify();
            });
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
            let collapsed = assert_content_anchor(cx, window, "toast-stack", dimensions, true);
            let header = e2e::control_bounds(window.window_id(), "ews-header")
                .expect("Editor header painted independently of center content");
            let more = i18n::Msg::ToastMore.t().replace("{count}", "3");
            assert!(crate::native_ax::press(cx, window, &more));
            cx.run_until_parked();
            let expanded = assert_content_anchor(cx, window, "toast-stack", dimensions, true);
            assert!(
                expanded.top() >= header.bottom(),
                "Editor header stays above notifications"
            );
            let banner = e2e::control_bounds(window.window_id(), "ews-external-changed")
                .expect("external-change banner painted");
            assert!(
                expanded.top() >= banner.bottom(),
                "Reload/Overwrite remain above notifications"
            );
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
                "Editor expansion retains the content bottom anchor"
            );
            assert!(
                expanded.bottom() <= editor_tabs.top(),
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
                    assert_content_anchor(cx, window, "toast-stack", dimensions, true);
                    let header = e2e::control_bounds(window.window_id(), "graph-column-header")
                        .expect("Graph column header painted");
                    assert!(
                        stack.top() >= header.bottom(),
                        "Graph header remains above feedback"
                    );
                    if let Some(sidebar) = e2e::control_bounds(window.window_id(), "sidebar-clip") {
                        assert!(stack.left() >= sidebar.right(), "Graph navigator excluded");
                    }
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
                        "expansion keeps its content bottom anchor"
                    );
                    assert_content_anchor(cx, window, "toast-stack", dimensions, true);
                    if kagi_domain::compact::is_compact(
                        dimensions.0,
                        theme::rem_size_px(),
                        theme::compact_threshold_rem(),
                    ) {
                        let tabs = e2e::control_bounds(window.window_id(), "tab-oplog").unwrap();
                        assert!(
                            expanded.bottom() <= tabs.top(),
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
                    let selected_stack =
                        assert_content_anchor(cx, window, "toast-stack", dimensions, true);
                    if let Some(inspector) =
                        e2e::control_bounds(window.window_id(), "right-pane-clip")
                    {
                        assert!(
                            selected_stack.right() <= inspector.left(),
                            "Inspector excluded"
                        );
                    }
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
                    // Keep the real repository failure; retire only app-level
                    // geometry fixtures before adding independent Settings
                    // fixtures within the shared four-notice budget.
                    app.update(cx, |app, cx| {
                        app.toast_stack.as_ref().unwrap().update(cx, |stack, cx| {
                            let fixtures: Vec<_> = stack
                                .toasts()
                                .iter()
                                .filter(|toast| toast.subject == ToastSubject::App)
                                .map(|toast| toast.id)
                                .collect();
                            for id in fixtures {
                                stack.remove(id);
                            }
                            assert_eq!(stack.toasts().len(), 1);
                            assert_eq!(stack.toasts()[0].subject, ToastSubject::Repository);
                            for n in 0..3 {
                                stack.push_keyed_notify(
                                    kagi::ui::toast_stack::ToastSpec::new(
                                        ToastSubject::Settings,
                                        kagi::ui::ToastKind::Error,
                                        format!("Settings layout fixture {n}"),
                                    ),
                                    cx,
                                );
                            }
                        });
                    });
                    // The native host shares storage, never notification subjects.
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
                    let settings_stack = assert_content_anchor(
                        cx,
                        settings,
                        "toast-stack",
                        settings_dimensions,
                        false,
                    );
                    let controls = HostControls::Settings.bounds(cx, settings);
                    assert!(settings_stack.size.height > px(0.));
                    let settings_more = i18n::Msg::ToastMore.t().replace("{count}", "2");
                    assert!(crate::native_ax::press(cx, settings, &settings_more),
                            "Settings disclosure {dimensions:?}/{zoom}/{preset}/{lang:?}/{distinct}: {}",
                            crate::native_ax::tree(cx, settings));
                    cx.run_until_parked();
                    draw(cx, settings);
                    let settings_expanded = assert_content_anchor(
                        cx,
                        settings,
                        "toast-stack",
                        settings_dimensions,
                        false,
                    );
                    assert!(settings_expanded.top() <= settings_stack.top());
                    assert!(
                        (settings_expanded.bottom() - settings_stack.bottom()).abs() <= px(1.),
                        "Settings expansion grows upward from its content-bottom anchor"
                    );
                    assert_eq!(HostControls::Settings.bounds(cx, settings), controls);
                    assert!(crate::native_ax::press(
                        cx,
                        settings,
                        i18n::Msg::ToastCollapse.t()
                    ));
                    // AX Press delivers the request before the collapse and
                    // Settings observer have settled. Drain those first, then
                    // settle draw-triggered native fitting as in settings_window.
                    // Busy feedback must be compared with the collapsed lane,
                    // not with the previous expanded frame.
                    cx.run_until_parked();
                    for _ in 0..4 {
                        draw(cx, settings);
                        crate::macos::drain_native_events();
                        cx.run_until_parked();
                    }
                    draw(cx, settings);
                    let settings_controls_before_busy = HostControls::Settings.bounds(cx, settings);
                    let settings_lane_before_busy =
                        e2e::control_bounds(settings.window_id(), "toast-content-anchor").unwrap();
                    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
                    KagiApp::hold_next_fetch_for_e2e(hold);
                    app.update(cx, |app, cx| app.fetch_async(false, cx));
                    cx.run_until_parked();
                    for (host, repository, host_dimensions) in [
                        (window, true, dimensions),
                        (settings, false, settings_dimensions),
                    ] {
                        e2e::clear_control_bounds(host.window_id(), "busy-snackbar");
                        draw(cx, host);
                        if repository {
                            assert_content_anchor(cx, host, "busy-snackbar", host_dimensions, true);
                        } else {
                            assert!(
                                e2e::control_bounds(host.window_id(), "busy-snackbar").is_none(),
                                "repository fetch progress never enters Settings"
                            );
                            assert_eq!(
                                HostControls::Settings.bounds(cx, host),
                                settings_controls_before_busy
                            );
                            assert_eq!(
                                e2e::control_bounds(host.window_id(), "toast-content-anchor")
                                    .unwrap(),
                                settings_lane_before_busy
                            );
                        }
                        assert_content_anchor(cx, host, "toast-stack", host_dimensions, repository);
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
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.bottom_panel_open = false;
            app.bottom_tab = kagi::ui::BottomTab::Terminal;
        });
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
                        if active {
                            matches!(&state.status_footer,
                                kagi::ui::FooterStatus::Idle(text) if !text.is_empty())
                        } else {
                            state
                                .toast_stack
                                .as_ref()
                                .unwrap()
                                .read(cx)
                                .toasts()
                                .iter()
                                .any(|toast| {
                                    toast.kind == kagi::ui::ToastKind::Sync
                                        && toast.result_key.is_some()
                                        && toast.born >= requested_at
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
                        "Push must leave the initially closed bottom panel closed"
                    );
                    assert_eq!(
                        state.bottom_tab,
                        kagi::ui::BottomTab::Terminal,
                        "Push must not select Operation Log"
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
                    assert!(
                        app.fetch_async_for(false, None, cx),
                        "manual Fetch is admitted"
                    );
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
                    "{lang:?}/{destination}: background failure presents its detail action"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            let entries = kagi_git::read_oplog_tail_for_repo(&repo, 100);
            assert_eq!(
                entries.iter().filter(|entry| entry.op == "fetch").count(),
                1,
                "the departed manual Fetch retains exactly one durable receipt"
            );
            let recorded = entries
                .into_iter()
                .find(|entry| entry.op == "fetch")
                .expect("real failed Fetch is durable");
            // The first durable receipt legitimately has ID zero. Verify its
            // complete identity through the actual detail action below.
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
            let preview = cx.read(|cx| {
                let state = app.read(cx);
                assert!(!state.bottom_panel_open, "failure never auto-opens details");
                let stack = state.toast_stack.as_ref().unwrap().read(cx);
                assert_eq!(stack.toasts().len(), 1, "one departed workflow result");
                let toast = &stack.toasts()[0];
                assert_eq!(toast.kind, kagi::ui::ToastKind::Error);
                assert!(toast.result_key.is_some() && toast.action.is_some());
                assert!(!toast.message.contains(repo.to_str().unwrap()));
                assert!(!toast.message.contains("missing-remote"));
                toast.message.clone()
            });
            if lang == i18n::Lang::Ja {
                crate::native_ax::activate(cx);
            }
            draw(cx, window);
            let action = e2e::control_bounds(window.window_id(), "operation-result-details")
                .expect("failure detail action is actually drawn on this screen");
            if lang == i18n::Lang::Ja {
                assert!(crate::native_ax::press_in(
                    cx,
                    window,
                    &preview,
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
                let revealed = &panel.entries()[expanded];
                assert_eq!(revealed.id, recorded.id);
                assert_eq!(revealed.timestamp, recorded.timestamp);
                assert_eq!(revealed.op, recorded.op);
                assert_eq!(revealed.repo, recorded.repo);
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

#[derive(Clone, Copy)]
enum HostControls {
    Settings,
    Home,
    Takeover,
}

impl HostControls {
    fn bounds(self, cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Bounds<Pixels> {
        match self {
            Self::Settings => cx
                .update_window(window, |_, window, cx| {
                    let view =
                        kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx)
                            .expect("actual shared Settings host");
                    view.read(cx)
                        .control_probe("settings-theme-apple-light")
                        .expect("actual Appearance theme card")
                        .bounds
                })
                .unwrap(),
            Self::Home => e2e::control_bounds(window.window_id(), "home-page-controls").unwrap(),
            Self::Takeover => {
                e2e::control_bounds(window.window_id(), "takeover-page-controls").unwrap()
            }
        }
    }
}

fn assert_mixed_lane_stability(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    host_controls: HostControls,
    controls: Bounds<Pixels>,
    lane: Bounds<Pixels>,
) {
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let (audience, subject) = match host_controls {
        HostControls::Settings => (ToastAudience::Settings, ToastSubject::Settings),
        HostControls::Home => (ToastAudience::Repository, ToastSubject::App),
        HostControls::Takeover => (ToastAudience::Repository, ToastSubject::Repository),
    };
    let stable = |cx: &mut VisualTestAppContext| {
        cx.run_until_parked();
        e2e::clear_control_bounds(window.window_id(), "busy-snackbar");
        draw(cx, window);
        assert_eq!(
            host_controls.bounds(cx, window),
            controls,
            "transient and busy feedback must not move content controls"
        );
        assert_eq!(
            e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap(),
            lane,
            "notification arrivals do not change the owning content bounds"
        );
    };
    for kind in [kagi::ui::ToastKind::Info, kagi::ui::ToastKind::Success] {
        let id = stack.update(cx, |stack, cx| {
            let id = stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    subject,
                    kind,
                    "Mixed-lane transient fixture",
                ),
                cx,
            );
            assert!(stack.toasts().iter().any(|toast| toast.is_persistent()));
            id
        });
        stable(cx);
        let containing = matches!(host_controls, HostControls::Takeover).then(|| Bounds {
            origin: gpui::point(lane.left(), lane.top()),
            size: gpui::size(lane.size.width, controls.bottom() - lane.top()),
        });
        assert_transient_overlay(cx, window, "toast-stack", containing);
        stack.update(cx, |stack, cx| {
            stack.advance_age_for_e2e(Duration::from_secs(60));
            assert!(stack.expiring_ids().contains(&id));
            stack.begin_exit(id, cx);
        });
        stable(cx);
        cx.advance_clock(Duration::from_secs(1));
        stable(cx);
        assert!(cx.read(|cx| stack.read(cx).toasts().iter().all(|toast| toast.id != id)));
    }
    let (read, release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_snapshot_for_e2e(read);
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    crate::busy_dismiss::advance(cx, 10);
    stable(cx);
    if audience == ToastAudience::Repository {
        assert!(
            e2e::control_bounds(window.window_id(), "busy-snackbar").is_some(),
            "held repository read remains visible in its owning window"
        );
        let containing = matches!(host_controls, HostControls::Takeover).then(|| Bounds {
            origin: gpui::point(lane.left(), lane.top()),
            size: gpui::size(lane.size.width, controls.bottom() - lane.top()),
        });
        assert_transient_overlay(cx, window, "busy-snackbar", containing);
    } else {
        assert!(
            e2e::control_bounds(window.window_id(), "busy-snackbar").is_none(),
            "repository progress must not leak into an unrelated Settings window"
        );
    }
    release.send(());
    crate::busy_dismiss::ready(cx, app);
    stable(cx);
}

fn takeover_lane_policy(cx: &mut VisualTestAppContext) {
    for mode in ["cleanup", "pr", "issues", "history", "diff"] {
        let fixture = build_fixture();
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| match mode {
            "cleanup" => app.open_branch_cleanup_view(cx),
            "pr" => app.show_pr_mode(cx),
            "issues" => {
                app.show_issues_mode(cx);
                app.show_empty_issues_for_e2e(cx);
            }
            "history" => app.open_file_history("README.md".into(), None, cx),
            "diff" => {
                app.select_headless(0);
                app.open_main_diff_commit(0, cx);
            }
            _ => unreachable!(),
        });
        cx.run_until_parked();
        draw(cx, window);
        let original = e2e::control_bounds(window.window_id(), "takeover-page-controls")
            .expect("takeover content painted");
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        let (read, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_snapshot_for_e2e(read);
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        crate::busy_dismiss::advance(cx, 10);
        draw(cx, window);
        assert!(e2e::control_bounds(window.window_id(), "busy-snackbar").is_some());
        assert!(!cx.read(|cx| stack
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.is_persistent())));
        assert_eq!(
            e2e::control_bounds(window.window_id(), "takeover-page-controls").unwrap(),
            original,
            "{mode}: busy feedback reserves no band"
        );
        release.send(());
        crate::busy_dismiss::ready(cx, &app);
        for kind in [kagi::ui::ToastKind::Info, kagi::ui::ToastKind::Success] {
            stack.update(cx, |stack, cx| {
                stack.push_notify(kind, "Ephemeral takeover fixture", cx);
                assert!(!stack.toasts().iter().any(|toast| toast.is_persistent()));
            });
            cx.run_until_parked();
            draw(cx, window);
            assert_eq!(
                e2e::control_bounds(window.window_id(), "takeover-page-controls").unwrap(),
                original,
                "{mode}: ephemeral feedback reserves no band"
            );
            let toast = e2e::control_bounds(window.window_id(), "toast-stack").unwrap();
            assert!(toast.left() >= original.left() && toast.right() <= original.right());
        }
        let error = stack.update(cx, |stack, cx| {
            stack.push_notify(
                kagi::ui::ToastKind::Error,
                "Persistent takeover fixture",
                cx,
            );
            stack.toasts().last().unwrap().id
        });
        cx.run_until_parked();
        draw(cx, window);
        let lane = e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap();
        let shifted = e2e::control_bounds(window.window_id(), "takeover-page-controls").unwrap();
        assert_eq!(shifted, original, "persistent feedback never moves content");
        assert_mixed_lane_stability(cx, &app, window, HostControls::Takeover, shifted, lane);
        stack.update(cx, |stack, cx| stack.begin_exit(error, cx));
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(
            e2e::control_bounds(window.window_id(), "takeover-page-controls").unwrap(),
            original,
            "{mode}: dismissing the persistent card leaves no empty lane"
        );
        if matches!(mode, "pr" | "issues") {
            assert_nested_feedback_host(
                cx,
                &app,
                window,
                if mode == "pr" {
                    "pr-mode-center-pane"
                } else {
                    "issue-mode-center-pane"
                },
                false,
            );
        }
        unmount(cx, app, window);
    }
}

fn non_repository_lane_policy(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "settings_pane",
        "settings_window_frame",
        "settings_window_position",
        "settings_window_height",
    ]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    for settings in [false, true] {
        let subject = if settings {
            ToastSubject::Settings
        } else {
            ToastSubject::App
        };
        let fixture = build_fixture();
        let (app, repository) = mount(cx, fixture.path());
        let window = if settings {
            kagi::ui::settings::write_setting("settings_pane", Some("appearance"));
            cx.simulate_keystrokes(repository, "cmd-,");
            cx.run_until_parked();
            cx.read(|cx| cx.windows())
                .into_iter()
                .find(|window| window.window_id() != repository.window_id())
                .unwrap()
        } else {
            cx.update_window(repository, |_, window, cx| {
                app.update(cx, |app, cx| app.open_home_tab(window, cx));
            })
            .unwrap();
            repository
        };
        let host_controls = if settings {
            HostControls::Settings
        } else {
            HostControls::Home
        };
        draw(cx, window);
        let original = host_controls.bounds(cx, window);
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        stack.update(cx, |stack, cx| {
            stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    subject,
                    kagi::ui::ToastKind::Info,
                    "Transient layout fixture",
                ),
                cx,
            );
            assert!(!stack.toasts().iter().any(|toast| toast.is_persistent()));
        });
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(
            host_controls.bounds(cx, window).top(),
            original.top(),
            "Info cannot move first-row controls"
        );
        let transient = e2e::control_bounds(window.window_id(), "toast-stack").unwrap();
        let anchor = e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap();
        assert!((transient.left() - anchor.left() - theme::scaled_px(12.)).abs() <= px(1.));
        let error = stack.update(cx, |stack, cx| {
            stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    subject,
                    kagi::ui::ToastKind::Error,
                    "Persistent layout fixture",
                ),
                cx,
            )
        });
        cx.run_until_parked();
        draw(cx, window);
        let lane = e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap();
        let shifted = host_controls.bounds(cx, window);
        assert_eq!(shifted, original, "persistent feedback never moves content");
        assert_mixed_lane_stability(cx, &app, window, host_controls, shifted, lane);
        if settings {
            let (sidebar, title) = cx
                .update_window(window, |_, window, cx| {
                    let view =
                        kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx)
                            .unwrap();
                    let view = view.read(cx);
                    (
                        view.control_probe("settings-sidebar").unwrap().bounds,
                        view.control_probe("settings-pane-title").unwrap().bounds,
                    )
                })
                .unwrap();
            assert!(
                lane.left() >= sidebar.right(),
                "anchor excludes the Settings sidebar"
            );
            assert!(
                lane.top() >= title.bottom(),
                "anchor excludes the pane heading"
            );
        }
        stack.update(cx, |stack, cx| {
            stack.advance_age_for_e2e(Duration::from_secs(60));
            assert!(!stack.expiring_ids().contains(&error), "errors persist");
            stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    subject,
                    kagi::ui::ToastKind::Success,
                    "Later transient fixture",
                ),
                cx,
            );
            assert!(stack
                .toasts()
                .iter()
                .any(|toast| toast.id == error && toast.is_persistent()));
        });
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(
            host_controls.bounds(cx, window),
            shifted,
            "a later Success never changes content control positions"
        );
        assert_eq!(
            e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap(),
            lane
        );
        // Explicit dismissal uses the same state transition as ×/Escape;
        // button hit-testing is exercised by the budget matrix above.
        stack.update(cx, |stack, cx| stack.begin_exit(error, cx));
        cx.run_until_parked();
        draw(cx, window);
        assert!(cx.read(|cx| stack
            .read(cx)
            .toasts()
            .iter()
            .all(|toast| toast.id != error || toast.dismissing.is_some())));
        assert_eq!(
            host_controls.bounds(cx, window).top(),
            original.top(),
            "explicit dismissal closes lane even with transient feedback"
        );
        if settings {
            cx.update_window(window, |_, window, _| window.remove_window())
                .unwrap();
        }
        unmount(cx, app, repository);
    }
}

fn conflict_content_anchor(cx: &mut VisualTestAppContext) {
    let fixture = crate::app_conflict::content_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    conflict.update(cx, |view, cx| {
        let path = std::path::Path::new("file.txt");
        view.conflict_open_editor(path);
        view.conflict_editor_set_hunk_side(
            path,
            0,
            kagi_git::resolution::SelectionSide::Incoming,
            true,
        );
        cx.notify();
    });
    std::fs::write(fixture.path().join("file.txt"), "external edit\n").unwrap();
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let error = stack.update(cx, |stack, cx| {
        stack.push_notify(
            kagi::ui::ToastKind::Error,
            "Persistent conflict geometry fixture",
            cx,
        );
        stack.toasts().last().unwrap().id
    });
    cx.run_until_parked();
    draw(cx, window);
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    let bounds = assert_content_anchor(
        cx,
        window,
        "toast-stack",
        (f32::from(viewport.width), f32::from(viewport.height)),
        true,
    );
    let header = e2e::control_bounds(window.window_id(), "conflict-content-header").unwrap();
    let sidebar = e2e::control_bounds(window.window_id(), "conflict-sidebar-clip").unwrap();
    let reload = e2e::control_bounds(window.window_id(), "conflict-reload")
        .expect("real external-change help and Reload are shown");
    assert!(
        bounds.top() >= header.bottom(),
        "full header and help precede stack"
    );
    assert!(
        bounds.left() >= sidebar.right(),
        "conflict tree is excluded"
    );
    assert!(bounds.top() >= reload.bottom(), "Reload is never covered");
    let source_headers = e2e::control_bounds(window.window_id(), "conflict-source-headers")
        .expect("actual source-pane column headers painted");
    let anchor = e2e::control_bounds(window.window_id(), "toast-content-anchor").unwrap();
    assert!(
        anchor.top() >= source_headers.bottom(),
        "notification anchor excludes the source column headers"
    );
    assert!(
        bounds.top() >= source_headers.bottom(),
        "both source identities remain unobscured by persistent feedback"
    );
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(60));
        assert!(
            !stack.expiring_ids().contains(&error),
            "conflict error is persistent"
        );
    });
    unmount(cx, app, window);
}

fn assert_transient_overlay(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    control: &str,
    _containing: Option<Bounds<Pixels>>,
) {
    draw(cx, window);
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    let containing = e2e::control_bounds(window.window_id(), "toast-content-anchor")
        .expect("owning centre content is painted");
    let overlay = e2e::control_bounds(window.window_id(), "toast-container")
        .expect("shared notification container painted");
    assert!(
        (overlay.left() - containing.left() - theme::scaled_px(12.)).abs() <= px(2.),
        "transient overlay retains its leading inset"
    );
    assert!(
        (containing.bottom() - overlay.bottom() - theme::scaled_px(12.)).abs() <= px(2.),
        "all feedback shares the content-bottom inset"
    );
    assert!(overlay.right() <= containing.right());
    assert!(
        overlay.size.width <= theme::scaled_px(460.).min(containing.size.width * 0.45) + px(1.)
    );
    assert!(overlay.size.height <= viewport.height * 0.3 + px(1.));
    let feedback = e2e::control_bounds(window.window_id(), control)
        .unwrap_or_else(|| panic!("{control} is painted in its overlay"));
    assert!(feedback.size.width > px(0.) && feedback.size.height > px(0.));
    assert!(feedback.left() >= overlay.left() && feedback.right() <= overlay.right() + px(1.));
    assert!(feedback.top() >= overlay.top() && feedback.bottom() <= overlay.bottom() + px(1.));
}

fn assert_content_anchor(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    control: &str,
    dimensions: (f32, f32),
    repository: bool,
) -> Bounds<Pixels> {
    // A resize can replace the full navigator with the compact rail. Probes
    // retain their last paint, so discard old chrome geometry before measuring
    // this frame rather than intersecting a sidebar that is no longer mounted.
    let chrome_ids = [
        "sidebar-clip",
        "compact-rail",
        "conflict-sidebar-clip",
        "settings-titlebar",
        "right-pane-clip",
        "toolbar-row",
    ];
    for id in chrome_ids {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    draw(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), control)
        .unwrap_or_else(|| panic!("{control} painted in native host"));
    // measure_inside records the busy card's content box inside its 1px border.
    let border = if control == "busy-snackbar" {
        px(1.)
    } else {
        px(0.)
    };
    let anchor = e2e::control_bounds(window.window_id(), "toast-content-anchor")
        .expect("actual content-column anchor painted");
    assert!(
        (bounds.left() - anchor.left() - theme::scaled_px(12.) - border).abs() <= px(2.),
        "{control} stays at the content leading inset: {bounds:?}, {anchor:?}"
    );
    assert!(bounds.left() >= anchor.left());
    assert!(bounds.top() >= anchor.top());
    assert!(bounds.bottom() <= anchor.bottom() + px(1.));
    assert!(
        bounds.size.width <= theme::scaled_px(460.).min(anchor.size.width * 0.45) + px(1.),
        "{control} retains its actual center-column width cap"
    );
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    assert!(
        bounds.size.height <= viewport.height * 0.3 + px(1.),
        "{control} retains the current viewport budget"
    );
    let container = e2e::control_bounds(window.window_id(), "toast-container")
        .expect("one shared stack container");
    assert!((anchor.bottom() - container.bottom() - theme::scaled_px(12.)).abs() <= px(2.));
    assert!(
        container.top() >= anchor.top(),
        "stack fits actual content height"
    );
    for id in chrome_ids {
        if let Some(chrome) = e2e::control_bounds(window.window_id(), id) {
            assert!(
                container.right() <= chrome.left()
                    || container.left() >= chrome.right()
                    || container.bottom() <= chrome.top()
                    || container.top() >= chrome.bottom(),
                "notification stack never intersects {id}: stack {container:?}, chrome {chrome:?}"
            );
        }
    }
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

/// Compare with independent workspace probes, not the notification's own parent.
pub(crate) fn assert_nested_feedback_host(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    center_id: &str,
    require_lane: bool,
) {
    let (was_open, tab) = cx.read(|cx| (app.read(cx).bottom_panel_open, app.read(cx).bottom_tab));
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        cx.notify();
    });
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let ids = stack.update(cx, |stack, cx| {
        [kagi::ui::ToastKind::Error, kagi::ui::ToastKind::Info].map(|kind| {
            stack.push_keyed_notify(
                kagi::ui::toast_stack::ToastSpec::new(
                    ToastSubject::Repository,
                    kind,
                    "Nested host geometry",
                ),
                cx,
            )
        })
    });
    cx.run_until_parked();
    draw(cx, window);
    let content =
        e2e::control_bounds(window.window_id(), center_id).expect("independent centre pane");
    let panel =
        e2e::control_bounds(window.window_id(), "bottom-panel").expect("reopened bottom panel");
    let toast = e2e::control_bounds(window.window_id(), "toast-container").expect("mixed stack");
    assert!(toast.left() >= content.left() && toast.right() <= content.right());
    assert!(toast.top() >= content.top() && toast.bottom() <= content.bottom());
    assert!((toast.left() - content.left() - theme::scaled_px(12.)).abs() <= px(2.));
    assert!((content.bottom() - toast.bottom() - theme::scaled_px(12.)).abs() <= px(2.));
    assert!(
        toast.bottom() <= panel.top(),
        "notifications never occupy Terminal/Operation Log"
    );
    if require_lane {
        let lane = e2e::control_bounds(window.window_id(), "pr-mode-lane-pane")
            .expect("actual PR swimlane");
        assert!(
            toast.left() >= lane.right(),
            "notifications never occupy the swimlane"
        );
    }
    stack.update(cx, |stack, cx| {
        for id in ids {
            stack.begin_exit(id, cx);
        }
    });
    app.update(cx, |app, cx| {
        app.bottom_panel_open = was_open;
        app.bottom_tab = tab;
        cx.notify();
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
}
