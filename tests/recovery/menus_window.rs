//! Native window identity, stateful menus and the About task boundary.
use crate::macos::{build_fixture, git, unmount};
use gpui::Action as _;
use gpui::{AnyWindowHandle, Entity, MenuItem, Role, VisualTestAppContext};
use kagi::ui::{commands, dialog_a11y, e2e, window_menu, KagiApp};
use kagi_ui_core::i18n::{self, Lang};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn mount_short(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
    let out = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        gpui::size(gpui::px(1000.), gpui::px(720.)),
        move |window, cx| e2e::mount_root(state, window, cx, &out),
    );
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    cx.run_until_parked();
    (app, window.into())
}
fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, w, cx| {
        w.refresh();
        w.draw(cx).clear();
    })
    .unwrap();
}
fn invoke(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    id: &'static str,
) {
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |a, cx| a.handle_menu_command(id, w, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, win);
}
fn menu_labels(menus: Vec<gpui::Menu>, section: &str) -> Vec<String> {
    menus
        .into_iter()
        .find(|m| m.name.as_ref() == i18n::menu_heading(section, i18n::lang()))
        .unwrap()
        .items
        .into_iter()
        .filter_map(|i| match i {
            MenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect()
}
fn labels(cx: &mut VisualTestAppContext, section: &str) -> Vec<String> {
    menu_labels(cx.read(commands::build_menus), section)
}
fn title(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> String {
    cx.update_window(win, |_, w, _| w.window_title()).unwrap()
}
fn repo_title(repo: &std::path::Path, branch: &str) -> String {
    format!("{} — {branch}", repo.file_name().unwrap().to_string_lossy())
}

pub fn scenario_conflict_repository_menu(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let old_lang = i18n::lang();
    let fixture = crate::app_conflict::content_fixture();
    let repo = fixture.path();
    let (app, win) = mount_short(cx, repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    draw(cx, win);
    let available = |cx: &mut VisualTestAppContext, action: &dyn gpui::Action| {
        cx.update_window(win, |_, window, cx| window.is_action_available(action, cx))
            .unwrap()
    };
    for (lang, expected) in [
        (
            Lang::En,
            ["Abort Merge…", "Skip This Step…", "Review to Continue…"],
        ),
        (
            Lang::Ja,
            ["merge を中止…", "このステップをスキップ…", "確認へ進む…"],
        ),
    ] {
        i18n::set_lang(lang);
        draw(cx, win);
        let repository = labels(cx, "Repository");
        assert!(repository
            .windows(3)
            .any(|items| items.iter().map(String::as_str).eq(expected)));
    }
    assert!(available(cx, &commands::ConflictAbort));
    assert!(!available(cx, &commands::ConflictSkip), "merge has no Skip");
    assert!(
        !available(cx, &commands::ConflictContinue),
        "unresolved files block Continue"
    );
    let before = crate::macos::repo_fingerprint(repo);
    invoke(cx, &app, win, "repo.conflictAbort");
    cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_some()));
    assert!(
        !available(cx, &commands::ConflictAbort),
        "modal blocks a second action"
    );
    assert_eq!(
        crate::macos::repo_fingerprint(repo),
        before,
        "Abort only opens the existing confirmation"
    );
    app.update(cx, |app, cx| {
        app.cancel_conflict_abort();
        cx.notify();
    });
    invoke(cx, &app, win, "file.newTab");
    for action in [
        commands::ConflictAbort.boxed_clone(),
        commands::ConflictSkip.boxed_clone(),
        commands::ConflictContinue.boxed_clone(),
    ] {
        assert!(
            !available(cx, action.as_ref()),
            "Home has no mounted conflict menu actions"
        );
    }
    unmount(cx, app, win);

    git(repo, &["merge", "--abort"]);
    for (operation, en, ja) in [
        ("cherry-pick", "Abort Cherry-Pick…", "cherry-pick を中止…"),
        ("rebase", "Abort Rebase…", "rebase を中止…"),
        ("revert", "Abort Revert…", "revert を中止…"),
    ] {
        assert!(!git_fixture::git_succeeds(repo, &[operation, "feature"]));
        let (app, win) = mount_short(cx, repo);
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        draw(cx, win);
        assert!(cx
            .update_window(win, |_, window, cx| window
                .is_action_available(&commands::ConflictSkip, cx))
            .unwrap());
        for (lang, abort, skip, review) in [
            (Lang::En, en, "Skip This Step…", "Review to Continue…"),
            (Lang::Ja, ja, "このステップをスキップ…", "確認へ進む…"),
        ] {
            i18n::set_lang(lang);
            draw(cx, win);
            assert!(labels(cx, "Repository")
                .windows(3)
                .any(|items| items.iter().map(String::as_str).eq([abort, skip, review])));
        }
        let before = crate::macos::repo_fingerprint(repo);
        invoke(cx, &app, win, "repo.conflictSkip");
        assert_eq!(
            crate::macos::repo_fingerprint(repo),
            before,
            "first Skip only arms existing confirmation"
        );
        for (lang, expected) in [(Lang::En, "Confirm Skip"), (Lang::Ja, "スキップを確定")] {
            i18n::set_lang(lang);
            draw(cx, win);
            assert!(labels(cx, "Repository")
                .iter()
                .any(|label| label == expected));
        }
        unmount(cx, app, win);
        git(repo, &[operation, "--abort"]);
    }
    conflict_menu_saved_abort(cx);
    i18n::set_lang(old_lang);
}

/// A saved merge first exposes the read-owned fallback, then #1755 mounts its
/// overview. Menu and visible Abort share the checked-read/confirmation gates.
fn conflict_menu_saved_abort(cx: &mut VisualTestAppContext) {
    let fixture = crate::app_conflict::content_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("file.txt"), "resolved\n").unwrap();
    git(repo, &["add", "file.txt"]);
    let hold = crate::conflict_draft_inspection::DetectionHold::new(cx, false);
    let (app, win) = mount_short(cx, repo);
    hold.wait(cx, &app);
    draw(cx, win);
    cx.read(|cx| {
        assert!(app.read(cx).ui().conflict.is_none());
        assert!(app.read(cx).view().operation.is_some());
    });
    let available = |cx: &mut VisualTestAppContext| {
        cx.update_window(win, |_, window, cx| {
            window.is_action_available(&commands::ConflictAbort, cx)
        })
        .unwrap()
    };
    let before = crate::macos::repo_fingerprint(repo);
    assert!(
        !available(cx),
        "the pending checked read refuses menu Abort"
    );
    invoke(cx, &app, win, "repo.conflictAbort");
    cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_none()));
    assert_eq!(crate::macos::repo_fingerprint(repo), before);
    drop(hold);
    cx.run_until_parked();
    e2e::clear_control_bounds(win.window_id(), "operation-strip-abort");
    draw(cx, win);
    cx.read(|cx| assert!(app.read(cx).ui().conflict.is_some()));
    assert!(e2e::control_bounds(win.window_id(), "operation-strip-abort").is_none());
    assert!(e2e::control_bounds(win.window_id(), "conflict-abort").is_some());
    app.update(cx, |app, cx| {
        app.planning = Some("merge-plan");
        cx.notify();
    });
    draw(cx, win);
    assert!(!available(cx), "planning blocks menu Abort");
    invoke(cx, &app, win, "repo.conflictAbort");
    cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_none()));
    app.update(cx, |app, cx| {
        app.planning = None;
        cx.notify();
    });
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        draw(cx, win);
        assert!(available(cx));
        let abort = e2e::control_bounds(win.window_id(), "conflict-abort").unwrap();
        cx.simulate_click(win, abort.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_some()));
        app.update(cx, |app, cx| {
            app.cancel_conflict_abort();
            cx.notify();
        });
        draw(cx, win);
        invoke(cx, &app, win, "repo.conflictAbort");
        cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_some()));
        assert!(!available(cx), "modal blocks menu Abort too");
        assert_eq!(crate::macos::repo_fingerprint(repo), before);
        app.update(cx, |app, cx| {
            app.cancel_conflict_abort();
            cx.notify();
        });
    }
    app.update(cx, |app, cx| {
        e2e::await_conflict_revalidation(app);
        cx.notify();
    });
    draw(cx, win);
    assert!(!available(cx), "unadmitted observation blocks menu Abort");
    invoke(cx, &app, win, "repo.conflictAbort");
    cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_none()));
    assert_eq!(crate::macos::repo_fingerprint(repo), before);
    unmount(cx, app, win);

    git(repo, &["merge", "--abort"]);
    let (app, win) = mount_short(cx, repo);
    draw(cx, win);
    assert!(
        !cx.update_window(win, |_, window, cx| {
            window.is_action_available(&commands::ConflictAbort, cx)
        })
        .unwrap(),
        "no operation means no Abort"
    );
    invoke(cx, &app, win, "repo.conflictAbort");
    cx.read(|cx| assert!(app.read(cx).conflict_abort_modal().is_none()));
    unmount(cx, app, win);
}
pub fn scenario_window_title_identity(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    let linked = repo.join("linked");
    git(
        &repo,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let (app, win) = mount_short(cx, &repo);
    draw(cx, win);
    assert_eq!(
        title(cx, win),
        repo_title(&repo, "main"),
        "native title identifies the active repository and branch"
    );
    git(&repo, &["checkout", "-b", "topic"]);
    invoke(cx, &app, win, "file.refresh");
    assert_eq!(title(cx, win), repo_title(&repo, "topic"));
    app.update(cx, |a, cx| a.open_repository(linked.clone(), cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&linked, "linked"));
    app.update(cx, |a, cx| a.open_repository(other_repo.clone(), cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&other_repo, "main"));
    invoke(cx, &app, win, "file.newTab");
    assert_eq!(title(cx, win), "Kagi");
    invoke(cx, &app, win, "file.closeTab");
    assert_eq!(title(cx, win), repo_title(&other_repo, "main"));
    app.update(cx, |a, cx| a.switch_repo(0, cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&repo, "topic"));
    unmount(cx, app, win);
}
pub fn scenario_view_menu_state_labels(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "sidebar_visible"]);
    let old = i18n::lang();
    let fixture = build_fixture();
    let other = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    let (other_app, other_win) = mount_short(cx, other.path());
    let menus = cx.read(commands::build_menus);
    let view = menus
        .iter()
        .find(|menu| menu.name.as_ref() == "View")
        .unwrap();
    for submenu in view.items.iter().filter_map(|item| match item {
        MenuItem::Submenu(menu) => Some(menu),
        _ => None,
    }) {
        let mut selected = 0;
        for item in &submenu.items {
            if let MenuItem::Action { name, checked, .. } = item {
                assert!(
                    !name.starts_with('✓') && !name.starts_with(' '),
                    "menu names must not encode selection: {name:?}"
                );
                selected += usize::from(*checked);
            }
        }
        assert_eq!(selected, 1, "native selection for {}", submenu.name);
    }
    cx.update_window(win, |_, window, cx| {
        window.dispatch_action(Box::new(kagi::ui::FocusSidebarFilter), cx)
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, win);
    let menus = cx.read(commands::build_menus);
    let find = menus
        .iter()
        .flat_map(|menu| &menu.items)
        .find_map(|item| match item {
            MenuItem::Submenu(menu) if menu.name.as_ref() == "Find" => Some(menu),
            _ => None,
        })
        .unwrap();
    let MenuItem::Action { action, .. } = &find.items[0] else {
        panic!("Find action")
    };
    assert!(
        !cx.update_window(win, |_, window, cx| window
            .is_action_available(action.as_ref(), cx))
            .unwrap(),
        "Find must be unavailable in the non-searchable sidebar Input"
    );
    for (lang, hide, show, enter, exit) in [
        (
            Lang::En,
            "Hide Sidebar",
            "Show Sidebar",
            "Enter Full Screen",
            "Exit Full Screen",
        ),
        (
            Lang::Ja,
            "サイドバーを非表示",
            "サイドバーを表示",
            "フルスクリーンにする",
            "フルスクリーンを解除",
        ),
    ] {
        i18n::set_lang(lang);
        app.update(cx, |a, cx| {
            a.sidebar.visible = true;
            a.set_window_active_for_e2e(true, cx);
        });
        draw(cx, win);
        let [graph, prs, editor] = match lang {
            Lang::En => ["Graph", "Pull Requests", "Editor"],
            Lang::Ja => ["グラフ", "Pull Request", "エディター"],
        };
        for (id, label) in [
            ("view.showGraph", graph),
            ("view.togglePrMode", prs),
            ("view.toggleEditorWorkspace", editor),
            ("view.showGraph", graph),
        ] {
            invoke(cx, &app, win, id);
            let menus = cx.read(commands::build_menus);
            let view = menus
                .iter()
                .find(|menu| menu.name.as_ref() == i18n::menu_heading("View", lang))
                .unwrap();
            assert!(
                view.items.iter().any(|item| matches!(
                    item, MenuItem::Action { name, checked: true, .. } if name.as_ref() == label
                )),
                "active workspace {id} is checked"
            );
            assert_eq!(
                view.items
                    .iter()
                    .filter(|item| matches!(
                        item, MenuItem::Action { name, checked: true, .. }
                            if [graph, prs, editor].contains(&name.as_ref())
                    ))
                    .count(),
                1,
                "workspace selection is exclusive"
            );
        }
        assert!(
            labels(cx, "View").contains(&hide.to_string()),
            "visible sidebar offers {hide}"
        );
        assert!(labels(cx, "View").contains(&enter.to_string()));
        invoke(cx, &app, win, "view.toggleSidebar");
        assert!(!cx.read(|cx| app.read(cx).sidebar.visible));
        assert!(labels(cx, "View").contains(&show.to_string()));
        other_app.update(cx, |a, cx| {
            a.sidebar.visible = true;
            a.set_window_active_for_e2e(true, cx);
        });
        draw(cx, other_win);
        assert!(
            labels(cx, "View").contains(&hide.to_string()),
            "window B owns its own sidebar label"
        );
        draw(cx, win);
        assert!(
            labels(cx, "View").contains(&show.to_string()),
            "returning to A restores A's label"
        );
        app.update(cx, |a, cx| a.set_window_active_for_e2e(false, cx));
        other_app.update(cx, |a, cx| a.set_window_active_for_e2e(false, cx));
        assert!(cx.read(|cx| cx.active_window().is_none()));
        draw(cx, other_win);
        assert!(
            labels(cx, "View").contains(&show.to_string()),
            "inactive renders retain the last foreground menu owner"
        );
        let fullscreen = commands::menus_for_state(window_menu::ViewMenuState {
            sidebar_visible: false,
            fullscreen: true,
            ..Default::default()
        });
        assert!(
            menu_labels(fullscreen, "View").contains(&exit.to_string()),
            "OS fullscreen state offers the exit action"
        );
        assert_eq!(
            commands::effective_keystroke("view.fullScreen").as_deref(),
            Some("ctrl-cmd-f")
        );
    }
    i18n::set_lang(old);
    unmount(cx, other_app, other_win);
    unmount(cx, app, win);
}
pub fn scenario_window_menu_items(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang", "window_frame"]);
    kagi_ui_core::settings::write_setting("recent_repos", None);
    let fixture = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    let menus = cx.read(commands::build_menus);
    let file = menus
        .iter()
        .find(|menu| menu.name.as_ref() == "File")
        .unwrap();
    assert!(
        file.items.iter().any(|item| matches!(
            item, MenuItem::Submenu(menu) if menu.name.as_ref() == "Open Recent"
        )),
        "File menu must expose recently opened repositories"
    );
    assert_eq!(
        labels(cx, "Window"),
        [
            "Minimize",
            "Zoom",
            "Show Previous Tab",
            "Show Next Tab",
            "Bring All to Front",
            "New Window",
            "Close Window"
        ]
    );
    assert!(cx.read(|cx| matches!(
        commands::command_state(app.read(cx), "window.previousTab"),
        commands::CommandState::Disabled(_)
    )));
    let second = build_fixture();
    let third = build_fixture();
    app.update(cx, |a, cx| {
        a.open_repository(second.path().to_path_buf(), cx);
        a.open_repository(third.path().to_path_buf(), cx);
    });
    cx.run_until_parked();
    invoke(cx, &app, win, "window.previousTab");
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 1);
    invoke(cx, &app, win, "window.nextTab");
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    cx.simulate_keystrokes(win, "cmd-shift-[");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 1);
    cx.simulate_keystrokes(win, "cmd-shift-]");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    cx.simulate_keystrokes(win, "cmd-{");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).active_tab),
        1,
        "native shifted bracket representation switches to the previous tab"
    );
    cx.simulate_keystrokes(win, "cmd-}");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    invoke(cx, &app, win, "file.newTab");
    invoke(cx, &app, win, "window.previousTab");
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    invoke(cx, &app, win, "window.nextTab");
    assert!(cx.read(|cx| app.read(cx).home_in_front()));
    let menus = cx.read(commands::build_menus);
    let find = menus
        .iter()
        .flat_map(|menu| &menu.items)
        .find_map(|item| match item {
            MenuItem::Submenu(menu) if menu.name.as_ref() == "Find" => Some(menu),
            _ => None,
        })
        .unwrap();
    assert!(
        matches!(&find.items[0], MenuItem::Action { disabled: true, .. }),
        "Home has no Find target"
    );
    assert_eq!(
        commands::effective_keystroke("window.previousTab").as_deref(),
        Some("secondary-shift-[")
    );
    assert_eq!(
        commands::effective_keystroke("window.nextTab").as_deref(),
        Some("secondary-shift-]")
    );
    assert_eq!(cx.read(window_menu::main_window_targets), vec![win]);
    check_window_owners(cx, &app, win);
    for lang in [Lang::En, Lang::Ja] {
        let old = i18n::lang();
        i18n::set_lang(lang);
        let recent = commands::recent_menu();
        let paths: Vec<_> = recent
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, action, .. }
                    if action.as_any().is::<commands::OpenRecentRepository>() =>
                {
                    Some(name.to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            [
                third.path().canonicalize().unwrap(),
                second.path().canonicalize().unwrap()
            ]
            .map(|path| path.to_string_lossy().into_owned())
        );
        i18n::set_lang(old);
    }
    // The portable dropdown belongs to the repository workspace, not Home.
    invoke(cx, &app, win, "window.previousTab");
    app.update(cx, |app, cx| {
        assert!(app.open_platform_menu_for_e2e("File", cx))
    });
    for width in [1000., 940.] {
        cx.update_window(win, |_, window, cx| {
            window.resize(gpui::size(gpui::px(width), gpui::px(720.)));
            window.bounds_changed(cx);
        })
        .unwrap();
        draw(cx, win);
        let bounds = |name| {
            e2e::control_bounds(win.window_id(), name)
                .unwrap_or_else(|| panic!("visible recent menu group: {name}"))
        };
        let heading = bounds("platform-menu-recent-heading");
        let first = bounds("platform-menu-recent-0");
        let clear = bounds("platform-menu-recent-3");
        let open = bounds("platform-menu-cmd-file.openRepository-primary");
        let next = bounds("platform-menu-cmd-file.openInTerminal");
        assert!(heading.bottom() <= first.top());
        assert!(first.bottom() < bounds("platform-menu-recent-separator").top());
        assert!(bounds("platform-menu-recent-separator").bottom() < clear.top());
        assert!(clear.bottom() < next.top());
        assert_eq!(
            first.left(),
            open.left(),
            "recent labels use the command check column"
        );
        assert_eq!(
            clear.left(),
            open.left(),
            "Clear Menu uses the command check column"
        );
    }
    cx.simulate_keystrokes(win, "escape");
    cx.run_until_parked();
    let recent = commands::recent_menu();
    let MenuItem::Action { action, .. } = &recent.items[1] else {
        panic!("second recent repository")
    };
    cx.update_window(win, |_, window, cx| {
        window.dispatch_action(action.boxed_clone(), cx)
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(
        cx.read(|cx| app.read(cx).repo_path.clone()),
        Some(second.path().canonicalize().unwrap())
    );
    let tabs = cx.read(|cx| app.read(cx).tabs.len());
    let recent = commands::recent_menu();
    let MenuItem::Action { action, .. } = recent.items.last().unwrap() else {
        panic!("Clear Menu")
    };
    assert!(
        cx.update_window(win, |_, window, cx| window
            .is_action_available(action.as_ref(), cx))
            .unwrap(),
        "Clear Menu is available with recent repositories"
    );
    cx.update_window(win, |_, window, cx| {
        window.dispatch_action(action.boxed_clone(), cx)
    })
    .unwrap();
    cx.run_until_parked();
    let empty = commands::recent_menu();
    assert_eq!(empty.items.len(), 1);
    assert!(matches!(
        &empty.items[0],
        MenuItem::Action { disabled: true, .. }
    ));
    draw(cx, win);
    assert!(
        !cx.update(|cx| cx.is_action_available(&commands::ClearRecentRepositories)),
        "Clear Menu must be unavailable with empty history"
    );
    assert!(
        !cx.update_window(win, |_, window, cx| window
            .is_action_available(&commands::ClearRecentRepositories, cx))
            .unwrap(),
        "empty history removes the window's Clear Menu handler"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).tabs.len()),
        tabs,
        "Clear Menu leaves open tabs intact"
    );
    assert!(second.path().join(".git").is_dir());
    app.update(cx, |app, cx| {
        assert!(app.open_repository(second.path().to_path_buf(), cx))
    });
    cx.run_until_parked();
    check_recent_settings_activation(cx, &app, win, second.path());
    let dock = commands::dock_menu_items();
    assert_eq!(
        dock.len(),
        1,
        "Dock lists repository entries only, never Clear Menu or separators"
    );
    let MenuItem::Action { action, name, .. } = &dock[0] else {
        panic!("Dock repository action")
    };
    assert!(action.as_any().is::<commands::OpenRecentRepository>());
    assert_eq!(
        name.as_ref(),
        second.path().canonicalize().unwrap().to_string_lossy()
    );
    unmount(cx, app, win);
    for deleted in [false, true] {
        let action = if deleted {
            deleted_recent_action()
        } else {
            action.boxed_clone()
        };
        // Unlike VisualTestAppContext::update, AsyncApp::update flushes deferred
        // effects even when no window exists, matching a native menu event.
        cx.read(|cx| cx.to_async()).update(|cx| {
            assert!(cx.windows().is_empty(), "last repository window is closed");
            assert!(
                cx.is_action_available(action.as_ref()),
                "Dock recent remains available without windows"
            );
            cx.dispatch_action(action.as_ref());
        });
        cx.run_until_parked();
        let reopened = cx.read(|cx| {
            let windows = cx.windows();
            assert_eq!(
                windows.len(),
                1,
                "Dock recent creates one repository window"
            );
            windows[0]
        });
        let host = cx
            .update_window(reopened, |_, window, cx| {
                let root = window.root::<gpui_component::Root>().flatten().unwrap();
                root.read(cx).view().clone().downcast::<KagiApp>().unwrap()
            })
            .unwrap();
        draw(cx, reopened);
        if deleted {
            assert_recent_open_error(cx, &host, reopened);
            assert!(cx.read(|cx| host.read(cx).tabs.is_empty()));
        } else {
            assert_eq!(
                cx.read(|cx| host.read(cx).repo_path.clone()),
                Some(second.path().canonicalize().unwrap())
            );
            assert_eq!(cx.read(|cx| host.read(cx).tabs.len()), 1);
        }
        assert_eq!(
            cx.read(window_menu::recent_activation_target),
            Some(reopened),
            "Dock recent requests activation of its new repository host"
        );
        unmount(cx, host, reopened);
    }
}

fn check_recent_settings_activation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    recent: &std::path::Path,
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_window_frame", "settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let fixture = build_fixture();
    let (other_app, other_win) = mount_short(cx, fixture.path());
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    draw(cx, other_win);
    invoke(cx, &other_app, other_win, "app.settings");
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|handle| *handle != win && *handle != other_win)
            .unwrap()
    });
    draw(cx, settings);
    let original_tabs = cx.read(|cx| app.read(cx).tabs.len());
    let dock = commands::dock_menu_items();
    let MenuItem::Action { action, .. } = &dock[0] else {
        panic!("recent action")
    };
    let deleted = deleted_recent_action();
    cx.update_window(settings, |_, window, cx| {
        window.dispatch_action(deleted, cx)
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, other_win);
    assert_recent_open_error(cx, &other_app, other_win);
    assert_eq!(
        cx.read(|cx| other_app.read(cx).tabs.len()),
        1,
        "failed open creates no tab"
    );
    cx.update_window(settings, |_, window, cx| {
        window.dispatch_action(action.boxed_clone(), cx)
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, other_win);
    assert_eq!(
        cx.read(|cx| other_app.read(cx).repo_path.clone()),
        Some(recent.canonicalize().unwrap()),
        "Open Recent from Settings uses the last active repository window"
    );
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), original_tabs);
    assert_eq!(
        cx.read(window_menu::recent_activation_target),
        Some(other_win),
        "Open Recent raises its repository host, not Settings"
    );
    cx.simulate_keystrokes(settings, "cmd-w");
    cx.run_until_parked();
    unmount(cx, other_app, other_win);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    draw(cx, win);
}

fn deleted_recent_action() -> Box<dyn gpui::Action> {
    let fixture = build_fixture();
    kagi::ui::tabs::record_recent_repo(&fixture.path().canonicalize().unwrap());
    let items = commands::dock_menu_items();
    let MenuItem::Action { action, .. } = &items[0] else {
        panic!("recent repository")
    };
    let action = action.boxed_clone();
    fixture.close().unwrap();
    action
}

fn assert_recent_open_error(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) {
    let message = cx.read(|cx| {
        let app = app.read(cx);
        let toast = app.toast_stack.as_ref().unwrap().read(cx).toasts().last().expect("open error toast");
        assert_eq!(toast.kind, kagi::ui::ToastKind::Error);
        assert!(matches!(&app.status_footer, kagi::ui::FooterStatus::Failed(text) if text == &toast.message));
        toast.message.clone()
    });
    let status = kagi::ui::toast_stack::status_for_e2e().expect("rendered failure notice");
    assert!(status
        .label
        .as_ref()
        .is_some_and(|label| label.contains(message.as_ref())));
    assert_eq!(
        cx.read(window_menu::recent_activation_target),
        Some(win),
        "failed Open Recent raises the host containing its error"
    );
}

fn check_window_owners(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_window_frame", "settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let other = build_fixture();
    let (other_app, other_win) = mount_short(cx, other.path());
    let other_tab = cx.read(|cx| other_app.read(cx).active_tab);
    invoke(cx, app, win, "app.settings");
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|handle| *handle != win && *handle != other_win)
            .expect("Settings utility host")
    });
    draw(cx, settings);
    assert_eq!(
        cx.read(window_menu::main_window_targets),
        vec![win, other_win],
        "Bring All to Front enumerates both main windows, never Settings"
    );
    cx.update_window(settings, |_, window, cx| {
        assert!(!window.is_action_available(&commands::PreviousTab, cx));
        assert!(!window.is_action_available(&commands::NextTab, cx));
        assert!(
            window
                .available_actions(cx)
                .iter()
                .any(|action| action.name() == commands::BringAllToFront.name()),
            "Bring All to Front is application-wide, including Settings"
        );
    })
    .unwrap();
    cx.simulate_keystrokes(settings, "cmd-shift-[");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).home_in_front()),
        "Settings cannot navigate repository tabs"
    );
    assert_eq!(cx.read(|cx| other_app.read(cx).active_tab), other_tab);
    cx.simulate_keystrokes(settings, "cmd-w");
    cx.run_until_parked();
    assert_eq!(
        title(cx, win),
        "Kagi",
        "Settings leaves the main title alone"
    );
    unmount(cx, other_app, other_win);
}
pub fn scenario_window_menu_front_order(cx: &mut VisualTestAppContext) {
    let first = build_fixture();
    let second = build_fixture();
    let (app, win) = mount_short(cx, first.path());
    let (other_app, other_win) = mount_short(cx, second.path());
    for handle in [other_win, win] {
        cx.update_window(handle, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
    }
    let before = cx.read(|cx| cx.window_stack().expect("native window stack"));
    assert_eq!(before, vec![win, other_win]);
    let key_before = cx.read(|cx| cx.active_window());
    let activation_order = cx.read(window_menu::bring_all_activation_order);
    assert_eq!(
        activation_order,
        vec![other_win, win],
        "raise back-to-front, restoring the invoking/frontmost window last"
    );
    assert_eq!(activation_order.last(), before.first());
    invoke(cx, &app, win, "window.bringAllToFront");
    assert_eq!(
        cx.read(|cx| cx.window_stack()),
        Some(before),
        "Bring All to Front preserves main-window front-to-back order"
    );
    assert_eq!(
        cx.read(|cx| cx.active_window()),
        key_before,
        "Bring All to Front preserves native key-window ownership"
    );
    unmount(cx, other_app, other_win);
    unmount(cx, app, win);
}
pub fn scenario_about_dialog(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let old_lang = i18n::lang();
    let old_theme = kagi_ui_core::theme::theme().slug.to_string();
    let old_zoom = kagi_ui_core::theme::zoom();
    let fixture = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    for slug in ["apple-light", "apple-dark"] {
        for zoom in [1., 1.67] {
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                kagi_ui_core::theme::set_zoom(zoom);
                cx.notify();
            });
            for (lang, name) in [(Lang::En, "About Kagi"), (Lang::Ja, "Kagiについて")] {
                i18n::set_lang(lang);
                check_about(cx, &app, win, name);
            }
        }
    }
    app.update(cx, |app, cx| {
        app.set_theme(&old_theme, cx);
        kagi_ui_core::theme::set_zoom(old_zoom);
        cx.notify();
    });
    i18n::set_lang(old_lang);
    unmount(cx, app, win);
}

fn check_about(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    name: &str,
) {
    dialog_a11y::clear_recorded_a11y();
    invoke(cx, app, win, "app.about");
    let spec = dialog_a11y::recorded_dialog("about-card")
        .expect("About is a named dialog, not an Info overlay");
    assert_eq!(spec.role, Role::Dialog);
    assert_eq!(spec.label, name);
    let card = e2e::control_bounds(win.window_id(), "about-card").expect("About frame");
    let close = e2e::control_bounds(win.window_id(), "about-close").expect("Close Button");
    assert!(card.top() >= gpui::px(0.) && card.bottom() <= gpui::px(480.));
    assert!(close.top() >= card.top() && close.bottom() <= card.bottom());
    let before = cx.read(|cx| app.read(cx).active_tab);
    for key in ["tab", "shift-tab", "tab", "tab"] {
        cx.simulate_keystrokes(win, key);
        cx.run_until_parked();
        assert!(
            cx.update_window(win, |_, w, cx| app
                .read(cx)
                .modal_focus
                .as_ref()
                .unwrap()
                .contains_focused(w, cx))
                .unwrap(),
            "{key} stays in About"
        );
    }
    cx.simulate_keystrokes(win, "escape");
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), before);
    assert!(cx.read(|cx| app.read(cx).about_modal().is_none()));
    invoke(cx, app, win, "app.about");
    cx.simulate_keystrokes(win, "enter");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).about_modal().is_none()),
        "Return closes only About"
    );
    invoke(cx, app, win, "app.about");
    close_about_button(cx, app, win);
}

fn close_about_button(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let close = e2e::control_bounds(win.window_id(), "about-close").expect("Close Button");
    cx.simulate_event(
        win,
        gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: close.center(),
            click_count: 1,
            ..Default::default()
        },
    );
    cx.simulate_event(
        win,
        gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: close.center(),
            click_count: 1,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).about_modal().is_none()),
        "Close Button dismisses About"
    );
}
