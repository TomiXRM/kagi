//! Real drawn list roles, Tab traversal, keyboard reveal and activation.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, list_a11y, theme, KagiApp};
use kagi_domain::github::{Issue, IssueListSnapshot, IssueState};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}
fn focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
) -> Option<String> {
    cx.update_window(window, |_, window, cx| {
        app.read(cx).list_row_focused_for_e2e(id, window)
    })
    .unwrap()
}
fn tab_into(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
) -> String {
    for _ in 0..120 {
        keys(cx, window, "tab");
        draw(cx, window);
        if let Some(key) = focused(cx, app, window, id) {
            return key;
        }
    }
    panic!("Tab did not reach {id}");
}
fn reveal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    list: &str,
    last: &str,
    row: &str,
    viewport: &str,
) {
    tab_into(cx, app, window, list);
    // Home/Issues are one stop: the next Tab leaves the entire list.
    if list != "branch-picker-list" {
        keys(cx, window, "tab");
        draw(cx, window);
        assert!(
            focused(cx, app, window, list).is_none(),
            "{list} has more than one Tab stop"
        );
        keys(cx, window, "shift-tab");
        draw(cx, window);
        assert!(focused(cx, app, window, list).is_some());
    }
    keys(cx, window, "end");
    draw(cx, window);
    assert_eq!(focused(cx, app, window, list).as_deref(), Some(last));
    let row = e2e::control_bounds(window.window_id(), row).expect("last row is drawn");
    let viewport = e2e::control_bounds(window.window_id(), viewport).expect("scroll viewport");
    assert!(
        row.top() >= viewport.top() - gpui::px(1.)
            && row.bottom() <= viewport.bottom() + gpui::px(1.),
        "focused row {row:?} outside {viewport:?}"
    );
    keys(cx, window, "up");
    draw(cx, window);
    keys(cx, window, "down");
    draw(cx, window);
    assert_eq!(focused(cx, app, window, list).as_deref(), Some(last));
    keys(cx, window, "home");
    draw(cx, window);
    keys(cx, window, "end");
    draw(cx, window);
}
fn assert_list(id: &str) {
    let list = list_a11y::recorded_list(id)
        .unwrap_or_else(|| panic!("{id} lacks named keyboard row semantics"));
    assert_eq!(list.role, Some(Role::List));
    assert!(!list.label.is_empty());
    assert!(!list.rows.is_empty());
}
fn matrix(
    cx: &mut VisualTestAppContext,
    mut check: impl FnMut(&mut VisualTestAppContext, (f32, f32), &'static str, Lang, f32),
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let old = (
        kagi_ui_core::i18n::lang(),
        theme::zoom(),
        theme::theme().slug.to_string(),
    );
    for size in [(940., 660.), (1392., 883.)] {
        for slug in ["apple-light", "apple-dark"] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    check(cx, size, slug, lang, zoom);
                }
            }
        }
    }
    kagi_ui_core::i18n::set_lang(old.0);
    theme::set_zoom(old.1);
    theme::set_active(&old.2);
    cx.update(theme::sync_gpui_component_theme);
}
fn configure(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    size: (f32, f32),
    slug: &str,
    lang: Lang,
    zoom: f32,
) {
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_theme(slug, cx);
    });
    crate::pr_detail_adaptive::resize(cx, window, size);
    draw(cx, window);
}
fn page() -> IssueListSnapshot {
    IssueListSnapshot {
        issues: (1..=30)
            .map(|number| Issue {
                number,
                title: format!("Issue 日本語 {{}} {number}"),
                state: IssueState::Open,
                url: format!("https://github.com/example/fixture/issues/{number}"),
                author: String::new(),
                assignees: Vec::new(),
                labels: Vec::new(),
                body: String::new(),
                comments: Vec::new(),
                comment_count: 0,
                created_at: String::new(),
                updated_at: format!("{:020}", 1000 - number),
            })
            .collect(),
        mentioned_numbers: Vec::new(),
        base_repo: "example/fixture".into(),
        next_cursor: None,
    }
}
fn issue_case(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    size: (f32, f32),
    slug: &str,
    lang: Lang,
    zoom: f32,
) {
    let (app, window) = mount(cx, repo);
    configure(cx, &app, window, size, slug, lang, zoom);
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page())));
    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    cx.run_until_parked();
    draw(cx, window);
    assert_list("issue-keyboard-list");
    issue_input_keys(cx, &app, window);
    reveal(
        cx,
        &app,
        window,
        "issue-keyboard-list",
        "30",
        "issue-main-row-30",
        "issue-main-list",
    );
    let metadata = list_a11y::recorded_list("issue-keyboard-list").unwrap();
    assert_eq!(
        metadata.rows[&29].0,
        kagi_ui_core::i18n::issue_row_name(
            30,
            "Issue 日本語 {} 30",
            kagi_ui_core::i18n::Msg::IssueStateOpen.t()
        )
    );
    issue_replacement(cx, &app, window);
    for activation in ["enter", "space"] {
        let _detail =
            e2e::queue_github_issue_detail(gpui::Task::ready(Ok(page().issues.pop().unwrap())));
        keys(cx, window, activation);
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .ui
                .values()
                .next()
                .unwrap()
                .selected_github_issue),
            Some(30)
        );
        assert!(
            focused(cx, &app, window, "issue-keyboard-list").is_none(),
            "hidden list releases focus"
        );
        if activation == "enter" {
            app.update(cx, |app, cx| {
                app.ui.values_mut().next().unwrap().selected_github_issue = None;
                cx.notify();
            });
            draw(cx, window);
            tab_into(cx, &app, window, "issue-keyboard-list");
            keys(cx, window, "end");
            draw(cx, window);
        }
    }
    unmount(cx, app, window);
}

fn issue_input_keys(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx))
    })
    .unwrap();
    for key in ["up", "down", "home", "end"] {
        keys(cx, window, key);
        draw(cx, window);
    }
    assert!(
        focused(cx, app, window, "issue-keyboard-list").is_none(),
        "Composer keys never navigate feed rows"
    );
    assert!(cx.read(|cx| app
        .read(cx)
        .ui
        .values()
        .next()
        .unwrap()
        .selected_github_issue
        .is_none()));
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().unwrap();
        root.focus(window, cx);
    })
    .unwrap();
    show_issue_rows(cx, window);
}

fn issue_replacement(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let mut replacement = page();
    replacement.issues.pop();
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(replacement)));
    app.update(cx, |app, cx| app.refresh_github_issues(cx));
    cx.run_until_parked();
    draw(cx, window);
    assert_ne!(
        focused(cx, app, window, "issue-keyboard-list").as_deref(),
        Some("30"),
        "replacement releases removed identity"
    );
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page())));
    app.update(cx, |app, cx| app.refresh_github_issues(cx));
    cx.run_until_parked();
    draw(cx, window);
    show_issue_rows(cx, window);
    tab_into(cx, app, window, "issue-keyboard-list");
    keys(cx, window, "end");
    draw(cx, window);
}

fn show_issue_rows(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    // At high zoom the Composer/filter can occupy the initial viewport.
    // Canonical RowFocus deliberately makes only a drawn row a Tab stop.
    draw(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), "issue-main-list").unwrap();
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-600.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, window);
}
pub fn scenario_issues_rows_keyboard(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    matrix(cx, |cx, size, slug, lang, zoom| {
        issue_case(cx, fixture.path(), size, slug, lang, zoom)
    });
    eprintln!("[gui-e2e] PASS issues_rows_keyboard");
}
fn seed_recents(paths: &[std::path::PathBuf]) {
    kagi::ui::settings::write_setting(
        "recent_repos",
        Some(
            &paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\u{1f}"),
        ),
    );
}
fn recent_case(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    paths: &[std::path::PathBuf],
    size: (f32, f32),
    slug: &str,
    lang: Lang,
    zoom: f32,
) {
    let (app, window) = mount(cx, repo);
    configure(cx, &app, window, size, slug, lang, zoom);
    seed_recents(paths);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
    assert_list("home-recent-list");
    let last = paths.last().unwrap().display().to_string();
    let row_id = format!("home-recent-{}", paths.len() - 1);
    reveal(
        cx,
        &app,
        window,
        "home-recent-list",
        &last,
        &row_id,
        "home-recent-scroll",
    );
    // Removal reconciles an exact path, not the shared basename or its old ordinal.
    if paths.len() > 1 {
        seed_recents(&paths[..paths.len() - 1]);
        draw(cx, window);
        assert_ne!(
            focused(cx, &app, window, "home-recent-list").as_deref(),
            Some(last.as_str())
        );
        seed_recents(paths);
        draw(cx, window);
        tab_into(cx, &app, window, "home-recent-list");
        keys(cx, window, "end");
        draw(cx, window);
    }
    if paths.len() > 1 {
        app.update(cx, |app, cx| app.close_home_tab(cx));
        cx.run_until_parked();
        draw(cx, window);
        assert!(
            focused(cx, &app, window, "home-recent-list").is_none(),
            "closing Home releases recent focus"
        );
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.open_home_tab(window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        draw(cx, window);
        tab_into(cx, &app, window, "home-recent-list");
        keys(cx, window, "end");
        draw(cx, window);
    }
    keys(cx, window, if lang == Lang::En { "enter" } else { "space" });
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).repo_path.clone()),
        Some(paths.last().unwrap().clone())
    );
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    assert!(focused(cx, &app, window, "home-recent-list").is_none());
    assert_eq!(
        cx.read(|cx| app.read(cx).tabs.len()),
        2,
        "activation opens the exact path once"
    );
    unmount(cx, app, window);
}
pub fn scenario_home_recent_rows_keyboard(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let fixture = build_fixture();
    let dirs: Vec<_> = (0..20).map(|_| tempfile::tempdir().unwrap()).collect();
    let paths: Vec<_> = dirs
        .iter()
        .map(|dir| {
            let path = dir.path().join("repo-日本語");
            let output = std::process::Command::new("git")
                .args(["clone", "--quiet"])
                .arg(fixture.path())
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            path.canonicalize().unwrap()
        })
        .collect();
    let _gh = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\ncase \"$1 $2\" in\n'config get') echo acme ;;\n*) echo '[]' ;;\nesac\n",
    );
    matrix(cx, |cx, size, slug, lang, zoom| {
        for count in [1, 20] {
            recent_case(cx, fixture.path(), &paths[..count], size, slug, lang, zoom);
        }
    });
    eprintln!("[gui-e2e] PASS home_recent_rows_keyboard");
}
fn invoke(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    command: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(command, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
}
fn branch_case(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    count: usize,
) {
    let last = if count == 1 {
        "main".to_owned()
    } else {
        format!("候補-{:03}", count - 2)
    };
    let row_id = format!("branch-pick-{}", count - 1);
    let head = git2::Repository::open(repo)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    for command in ["branch.checkout", "branch.delete", "branch.rename"] {
        let opener = cx
            .update_window(window, |_, window, cx| window.focused(cx))
            .unwrap();
        invoke(cx, app, window, command);
        assert_list("branch-picker-list");
        reveal(
            cx,
            app,
            window,
            "branch-picker-list",
            &last,
            &row_id,
            "branch-picker-scroll",
        );
        keys(cx, window, "escape");
        cx.run_until_parked();
        draw(cx, window);
        assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
        assert!(focused(cx, app, window, "branch-picker-list").is_none());
        let returned = cx
            .update_window(window, |_, window, cx| window.focused(cx))
            .unwrap();
        assert_eq!(
            returned, opener,
            "Escape returns focus to the valid picker opener"
        );
        for activation in ["enter", "space"] {
            invoke(cx, app, window, command);
            keys(cx, window, "end");
            draw(cx, window);
            keys(cx, window, activation);
            cx.run_until_parked();
            draw(cx, window);
            assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
            cx.read(|cx| {
                let app = app.read(cx);
                match command {
                    "branch.checkout" => assert!(matches!(&app.plan_modal().expect("checkout approval").target, kagi::ui::modals::CheckoutPlanTarget::Branch(name) if name == &last)),
                    "branch.delete" => assert_eq!(app.delete_branch_modal().expect("delete approval").branch_name, last),
                    _ => assert_eq!(app.rename_branch_modal().expect("rename form").old_name, last),
                }
            });
            let repository = git2::Repository::open(repo).unwrap();
            assert_eq!(
                repository.head().unwrap().target(),
                Some(head),
                "selection must not checkout before approval"
            );
            assert!(
                repository
                    .find_branch(&last, git2::BranchType::Local)
                    .is_ok(),
                "selection must not delete/rename before approval"
            );
            keys(cx, window, "escape");
            cx.run_until_parked();
            draw(cx, window);
        }
    }
}
pub fn scenario_branch_picker_keyboard(cx: &mut VisualTestAppContext) {
    let fixtures: Vec<_> = [1, 20, 200]
        .into_iter()
        .map(|count| {
            let fixture = build_fixture();
            let repo = git2::Repository::open(fixture.path()).unwrap();
            let commit = repo.head().unwrap().peel_to_commit().unwrap();
            for i in 0..count - 1 {
                repo.branch(&format!("候補-{i:03}"), &commit, false)
                    .unwrap();
            }
            fixture
        })
        .collect();
    matrix(cx, |cx, size, slug, lang, zoom| {
        for (fixture, count) in fixtures.iter().zip([1, 20, 200]) {
            let (app, window) = mount(cx, fixture.path());
            configure(cx, &app, window, size, slug, lang, zoom);
            branch_case(cx, &app, window, fixture.path(), count);
            unmount(cx, app, window);
        }
    });
    eprintln!("[gui-e2e] PASS branch_picker_keyboard");
}
