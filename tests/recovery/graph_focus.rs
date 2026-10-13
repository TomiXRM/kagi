//! Pointer focus handoff and foreground Graph copy ownership (#1447, #1448).
use crate::issue_conversation_selection::support;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::recovery_operations::paint;
use gpui::{px, AnyWindowHandle, Entity, Focusable, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{commit_list, e2e, i18n::Lang, settings, theme, KagiApp};

fn root_focus(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        focus.focus(window, cx);
    })
    .unwrap();
}
fn assert_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        assert!(
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window),
            "pointer-selected Graph must own keyboard focus, not the previous field/pane"
        );
    })
    .unwrap();
}
fn row_point(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> gpui::Point<gpui::Pixels> {
    paint(cx, win);
    // Subject bubbles to the commit row; the leftmost column contains ref chips
    // with their own branch activation/focus path.
    e2e::control_bounds(win.window_id(), "graph-commit-subject")
        .unwrap()
        .center()
}
fn click_row(cx: &mut VisualTestAppContext, win: AnyWindowHandle, button: MouseButton) {
    let at = row_point(cx, win);
    cx.simulate_mouse_down(win, at, button, Modifiers::none());
    cx.simulate_mouse_up(win, at, button, Modifiers::none());
    paint(cx, win);
}
fn copy(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> Option<String> {
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
        "foreground sentinel".into(),
    ));
    keys(cx, win, "secondary-c");
    cx.read_from_clipboard().and_then(|item| item.text())
}
fn copy_count() -> usize {
    kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.starts_with("[kagi] graph: copied "))
        .count()
}
fn assert_eligible(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    expected: bool,
) {
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            assert_eq!(
                app.commit_list_has_focus_for_e2e(window, cx),
                expected,
                "the production visible-center/front-layer model must agree with copy ownership"
            );
        });
    })
    .unwrap();
}
fn assert_copy(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    index: usize,
) {
    assert_eligible(cx, app, win, true);
    for target in ["hash", "branch"] {
        settings::write_setting("graph_copy_target", Some(target));
        let expected = cx.read(|cx| {
            let row = &app.read(cx).view().rows[index];
            commit_list::graph_copy_value(
                &row.badges,
                &row.id.0,
                settings::Settings::load().graph_copy_target(),
            )
        });
        let before = copy_count();
        assert_eq!(copy(cx, win).as_deref(), Some(expected.as_str()));
        assert_eq!(copy_count(), before + 1, "one copy logs exactly once");
    }
}

fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, size: (f32, f32)) {
    let wanted = gpui::size(px(size.0), px(size.1));
    cx.update_window(win, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(win, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native resize reaches {wanted:?}"
        );
    }
    crate::recovery_layout::draw(cx, win, size);
}

pub fn scenario_graph_pointer_focus(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _restore = support::Restore::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["graph_copy_target"]);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "filter-target", "HEAD~1"]);
    let before = repo_fingerprint(fixture.path());
    for size in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for lang in [Lang::En, Lang::Ja] {
                for slug in ["apple-light", "apple-dark"] {
                    let (app, win) = mount(cx, fixture.path());
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                    });
                    theme::set_zoom(zoom);
                    resize(cx, win, size);
                    pointer_case(cx, &app, win);
                    unmount(cx, app, win);
                }
            }
        }
    }
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "pointer selection performs no Git writes"
    );
}

fn pointer_case(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    keys(cx, win, "secondary-f");
    let input = cx.read(|cx| app.read(cx).sidebar.filter.clone()).unwrap();
    keys(cx, win, "f i l t e r - t a r g e t");
    let selection = cx.read(|cx| app.read(cx).ui().selected);
    keys(cx, win, "left down");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        selection,
        "field arrows cannot navigate Graph"
    );
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "filter-target"
    );
    cx.update_window(win, |_, window, cx| {
        assert!(
            input.read(cx).focus_handle(cx).is_focused(window),
            "typing/field arrows retain Input ownership until click"
        );
    })
    .unwrap();
    click_row(cx, win, MouseButton::Left);
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
    assert_root(cx, app, win);
    cx.update_window(win, |_, window, cx| {
        assert!(!input.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
    let before = e2e::control_bounds(win.window_id(), "graph-selection-bar").unwrap();
    keys(cx, win, "down");
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "filter-target"
    );
    assert_copy(cx, app, win, 1);
    paint(cx, win);
    let after = e2e::control_bounds(win.window_id(), "graph-selection-bar").unwrap();
    assert_eq!(
        before.size, after.size,
        "keyboard and pointer selection have identical geometry"
    );
    drop(input);
    other_pointer_owners(cx, app, win);
}

fn other_pointer_owners(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) {
    let (pane, key) = cx.read(|cx| {
        (0..5)
            .find_map(|pane| {
                app.read(cx)
                    .sidebar_row_keys_for_e2e(pane)
                    .into_iter()
                    .find(|key| key.contains("filter-target"))
                    .map(|key| (pane, key))
            })
            .unwrap()
    });
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(pane, &key, window, cx)
        });
    })
    .unwrap();
    click_row(cx, win, MouseButton::Left);
    assert_root(cx, app, win);
    // Reach a genuine rendered toolbar control through its keyboard traversal.
    let mut reached = false;
    for _ in 0..100 {
        keys(cx, win, "tab");
        e2e::take_focus_probe();
        keys(cx, win, "f19");
        if e2e::take_focus_probe().is_some() {
            reached = true;
            break;
        }
    }
    assert!(reached, "toolbar control receives keyboard focus");
    click_row(cx, win, MouseButton::Left);
    assert_root(cx, app, win);
    keys(cx, win, "secondary-f");
    click_row(cx, win, MouseButton::Right);
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
    keys(cx, win, "escape");
    paint(cx, win);
    assert_root(cx, app, win);
    keys(cx, win, "down");
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
    assert_copy(cx, app, win, 1);
}

pub fn scenario_graph_copy_foreground(cx: &mut VisualTestAppContext) {
    let _restore = support::Restore::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["graph_copy_target"]);
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, win) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    for takeover in ["cleanup", "pr", "issues", "history", "modal", "home"] {
        takeover_copy(cx, &app, win, takeover);
    }
    crate::app_conflict::click_control(cx, win, "repo-tab-0");
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    paint(cx, win);
    root_focus(cx, &app, win);
    assert_copy(cx, &app, win, 0);
    e2e::seed_diff_selection();
    assert_eq!(
        copy(cx, win).as_deref(),
        Some("selected"),
        "diff selected-text priority remains intact"
    );
    keys(cx, win, "escape");
    assert!(!e2e::diff_selection_present());
    foreground_text(cx, &app, win);
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "copy performs no Git writes"
    );
    unmount(cx, app, win);
}

fn takeover_copy(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    takeover: &str,
) {
    app.update(cx, |app, cx| {
        app.show_graph_mode(cx);
        match takeover {
            "cleanup" => app.open_branch_cleanup_view(cx),
            "pr" => app.show_pr_mode(cx),
            "issues" => app.show_empty_issues_for_e2e(cx),
            "history" => app.open_file_history("README.md".into(), None, cx),
            "modal" => {
                e2e::deliver_app_notice(app, "Foreground notice");
                cx.notify();
            }
            "home" => {}
            _ => unreachable!(),
        }
    });
    if takeover == "home" {
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| app.open_home_tab(window, cx))
        })
        .unwrap();
    }
    paint(cx, win);
    root_focus(cx, app, win);
    assert_root(cx, app, win);
    assert_eligible(cx, app, win, false);
    let before = copy_count();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "hidden selection survives {takeover}"
    );
    assert_eq!(
        copy(cx, win).as_deref(),
        Some("foreground sentinel"),
        "{takeover} must not copy the hidden Graph identity"
    );
    assert_eq!(
        copy_count(),
        before,
        "{takeover}: hidden Graph copy receipts = 0"
    );
    if takeover == "modal" {
        keys(cx, win, "escape");
    }
}

fn foreground_text(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let producer = support::Producer::install();
    producer.publish("a", "**Foreground text body**", &[]);
    support::configure(cx, app, support::BASE);
    support::load(cx, app, win);
    let body = support::measure(cx, win, support::BODY).unwrap();
    support::down(cx, win, support::start(body));
    support::move_to(cx, win, support::end(body));
    support::up(cx, win, support::end(body));
    root_focus(cx, app, win);
    assert_eligible(cx, app, win, false);
    let before = copy_count();
    assert_eq!(
        copy(cx, win).as_deref(),
        Some("Foreground text body"),
        "foreground TextView copy propagates, never hidden Graph"
    );
    assert_eq!(
        copy_count(),
        before,
        "foreground text never emits a Graph copy receipt"
    );
}
