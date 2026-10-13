use crate::macos::{build_fixture, git, unmount};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, i18n, list_a11y, theme, KagiApp};
use std::{cell::RefCell, rc::Rc};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn press(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
}
fn tab_to(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
    id: &str,
) {
    for _ in 0..120 {
        let focused = cx
            .update_window(win, |_, window, cx| {
                app.read(cx).dashboard_keyboard_focused_for_e2e(id, window)
            })
            .unwrap();
        if focused {
            return;
        }
        cx.simulate_keystrokes(win, "tab");
        draw(cx, win);
    }
    panic!("{id}: keyboard Tab never reaches production control");
}
fn queue_rows(reverse: bool, head: &str) {
    let mut rows: Vec<_> = (1..=80)
        .map(|n| {
            let mut pr = crate::evidence_support::pull_request(
                n,
                "Keyboard PR / キーボード",
                "feature/keys",
            );
            pr.head_sha = head.to_owned();
            pr
        })
        .collect();
    if reverse {
        rows.reverse();
    }
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
        rows, "", None,
    ))));
}
fn row_key(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
) -> String {
    cx.update_window(win, |_, window, cx| {
        app.read(cx).dashboard_row_focused_for_e2e(window).unwrap()
    })
    .unwrap()
}
fn rows(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
    head: &str,
) {
    tab_to(cx, app, win, "pr-home-list");
    cx.simulate_keystrokes(win, "home down");
    draw(cx, win);
    assert_eq!(row_key(cx, app, win), "2");
    cx.simulate_keystrokes(win, "pagedown");
    draw(cx, win);
    assert!(row_key(cx, app, win).parse::<u64>().unwrap() > 2);
    cx.simulate_keystrokes(win, "pageup end");
    draw(cx, win);
    assert_eq!(row_key(cx, app, win), "80");
    let viewport = e2e::control_bounds(win.window_id(), "pr-home-table-viewport").unwrap();
    let last = e2e::control_bounds(win.window_id(), "pr-home-row-80").unwrap();
    assert!(
        last.top() >= viewport.top() && last.bottom() <= viewport.bottom(),
        "End reveals row 80"
    );
    let ax = list_a11y::recorded_list("pr-list").unwrap();
    assert_eq!(ax.role, Some(gpui::Role::ListBox));
    let selected: Vec<_> = ax.rows.values().filter(|(_, selected)| *selected).collect();
    assert_eq!(selected.len(), 1);
    assert!(selected[0].0.contains("80"));
    queue_rows(true, head);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(
        row_key(cx, app, win),
        "80",
        "refresh/reorder keeps identity"
    );
    press(cx, win, "enter");
    cx.run_until_parked();
    draw(cx, win);
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        assert_eq!(mode.tabs[mode.active.unwrap()].pr.number, 80);
    });
    tab_to(cx, app, win, "pr-mode-refresh");
    let refresh = e2e::pr_detail::control("pr-mode-refresh").unwrap();
    assert_eq!(refresh.role, Some(gpui::Role::Button));
    assert_eq!(refresh.name.as_deref(), Some(i18n::Msg::PrRefresh.t()));
    queue_rows(false, head);
    press(cx, win, "space");
    cx.run_until_parked();
    draw(cx, win);
    tab_to(cx, app, win, "pr-mode-home");
    press(cx, win, "enter");
    draw(cx, win);
    cx.read(|cx| assert!(app.read(cx).pr_mode().unwrap().active.is_none()));
}
fn filters(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
    head: &str,
) {
    for id in [
        "list-filter-state",
        "list-filter-label",
        "list-filter-author",
        "list-filter-draft",
        "list-filter-checks",
        "list-filter-sort",
    ] {
        let control = e2e::pr_detail::control(id).unwrap();
        assert_eq!(control.role, Some(gpui::Role::Button));
        assert!(control.name.as_ref().is_some_and(|name| !name.is_empty()));
        for key in ["space", "enter"] {
            tab_to(cx, app, win, id);
            press(cx, win, key);
            draw(cx, win);
            cx.read(|cx| {
                assert!(
                    app.read(cx).dashboard_filter_menu_for_e2e(),
                    "{id}: {key} opens menu"
                )
            });
            cx.simulate_keystrokes(win, "escape");
            draw(cx, win);
            tab_to(cx, app, win, id);
            press(cx, win, key);
            draw(cx, win);
            cx.simulate_keystrokes(win, "home");
            press(cx, win, "enter");
            draw(cx, win);
            cx.read(|cx| {
                assert_eq!(
                    app.read(cx).dashboard_filter_menu_for_e2e(),
                    id == "list-filter-label",
                    "{id}: keyboard choice follows menu policy"
                )
            });
            if id == "list-filter-label" {
                cx.simulate_keystrokes(win, "escape");
                draw(cx, win);
            }
        }
    }
    filter_actions(cx, app, win, head);
}

fn filter_actions(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
    head: &str,
) {
    for id in [
        "list-filter-direction",
        "list-filter-clear",
        "list-filter-refresh",
    ] {
        for key in ["space", "enter"] {
            tab_to(cx, app, win, id);
            if id == "list-filter-clear" {
                tab_to(cx, app, win, "list-filter-sort");
                cx.simulate_keystrokes(win, "shift-tab keyboard");
                cx.run_until_parked();
                draw(cx, win);
                cx.read(|cx| {
                    assert_eq!(app.read(cx).ui().github_pr_filter.common.text, "keyboard")
                });
                tab_to(cx, app, win, id);
            }
            let before = cx.read(|cx| app.read(cx).ui().github_pr_filter.sort.direction);
            if id == "list-filter-refresh" {
                queue_rows(false, head);
            }
            press(cx, win, key);
            cx.run_until_parked();
            draw(cx, win);
            cx.read(|cx| {
                let filter = &app.read(cx).ui().github_pr_filter;
                if id == "list-filter-clear" {
                    assert!(filter.common.text.is_empty());
                }
                if id == "list-filter-direction" {
                    assert_ne!(filter.sort.direction, before);
                }
            });
        }
    }
}

fn empty_refresh(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    win: AnyWindowHandle,
    head: &str,
) {
    for failed in [false, true] {
        e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
            Vec::new(),
            "example/repo",
            None,
        ))));
        app.update(cx, |app, cx| app.refresh_github_prs(cx));
        cx.run_until_parked();
        draw(cx, win);
        if failed {
            e2e::queue_github_pr_fetch(gpui::Task::ready(Err(
                kagi_git::github::PrFetchError::Network("offline".into()),
            )));
            app.update(cx, |app, cx| app.refresh_github_prs(cx));
            cx.run_until_parked();
            draw(cx, win);
        }
        tab_to(cx, app, win, "pr-mode-refresh");
        let before = cx.read(|cx| app.read(cx).ui().github_prs_gen);
        queue_rows(false, head);
        press(cx, win, "space");
        cx.run_until_parked();
        draw(cx, win);
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).ui().github_prs_gen,
                before + 1,
                "one read per Space"
            );
            assert_eq!(app.read(cx).ui().github_prs.len(), 80);
        });
    }
}
fn scenario(cx: &mut VisualTestAppContext, filter: bool, dimensions: (f32, f32)) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let remote_dir = tempfile::tempdir().unwrap();
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    let remote = remote_dir.path().join("repo.git");
    git(
        fixture.path(),
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "HEAD:refs/heads/main",
            "HEAD:refs/pull/80/head",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(fixture.path(), &["remote", "add", "origin", url]);
    git(
        fixture.path(),
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    let head = crate::pr_viewed::rev(fixture.path(), "HEAD");
    let state = e2e::app_state(fixture.path()).unwrap();
    let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win: AnyWindowHandle = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow_mut().take().unwrap();
    queue_rows(false, &head);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    for lang in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", lang);
        i18n::init_lang();
        for zoom in [1., 1.667] {
            theme::set_zoom(zoom);
            app.update(cx, |_, cx| cx.notify());
            draw(cx, win);
            if filter {
                filters(cx, &app, win, &head);
            } else {
                rows(cx, &app, win, &head);
                empty_refresh(cx, &app, win, &head);
            }
        }
    }
    unmount(cx, app, win);
}
pub fn scenario_pr_dashboard_keyboard(cx: &mut VisualTestAppContext) {
    for dimensions in [(940., 660.), (1392., 883.)] {
        scenario(cx, false, dimensions);
    }
}
pub fn scenario_pr_filter_strip_keyboard(cx: &mut VisualTestAppContext) {
    for dimensions in [(940., 660.), (1392., 883.)] {
        scenario(cx, true, dimensions);
    }
}
