//! #1551: failed fetch stacks retain receipts without covering the workspace.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{px, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme};
use std::time::{Duration, Instant};

pub fn scenario_toast_stack_budget(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    for dimensions in [(1000., 720.), (1400., 900.)] {
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
                    theme::set_zoom(if dimensions.0 == 1000. { 1.67 } else { 1. });
                    app.update(cx, |app, cx| app.set_lang(lang, cx));
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
                    assert!(
                        stack.origin.y > px(dimensions.1 * 0.15),
                        "toolbar/search remain exposed"
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
                    let expanded = e2e::control_bounds(window.window_id(), "toast-stack").unwrap();
                    assert!(expanded.size.height <= px(dimensions.1 * 0.3 + 1.));
                    assert!(
                        crate::native_ax::press(cx, window, i18n::Msg::ToastCollapse.t()),
                        "named native collapse"
                    );
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
                    let tab = e2e::control_bounds(window.window_id(), "tab-oplog")
                        .expect("Operation Log tab");
                    cx.simulate_click(window, tab.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
                    let summary = cx
                        .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(0))
                        .expect("receipt row painted");
                    cx.simulate_click(
                        window,
                        summary.origin + gpui::point(px(40.), px(8.)),
                        gpui::Modifiers::none(),
                    );
                    cx.run_until_parked();
                    assert_eq!(
                        cx.read(|cx| panel.read(cx).expanded()),
                        Some(0),
                        "receipt details remain hit-testable"
                    );
                    drop(panel);
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
