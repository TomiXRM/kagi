//! #643 Wave 4 S2a: a Commit Panel file-menu intent freezes its session and
//! repository-relative path before crossing the deferred entity → root boundary.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{point, px, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, file_menu::FileMenu};

fn dirty_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("alpha.txt"), "alpha dirty\n").unwrap();
    std::fs::write(fixture.path().join("zeta.txt"), "zeta dirty\n").unwrap();
    fixture
}

fn defer_first_menu(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            e2e::defer_file_menu(app, 0, point(px(120.0), px(120.0)), window, cx);
        });
    })
    .unwrap();
}

fn draw_menu_and_bounds(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(window.window_id(), "file-menu-discard");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(window.window_id(), "file-menu-discard")
        .expect("the real Discard menu item was not laid out")
}

pub fn scenario_file_menu_freezes_path(cx: &mut VisualTestAppContext) {
    let fixture = dirty_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();

    let (owner, frozen, other) = cx.read(|cx| {
        let app = app.read(cx);
        let panel = app
            .ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .read(cx);
        assert_eq!(panel.state.unstaged.len(), 2, "fixture has two menu rows");
        (
            panel.owner,
            panel.state.unstaged[0].path.clone(),
            panel.state.unstaged[1].path.clone(),
        )
    });

    // Start the production deferred callback, then renumber its source rows
    // before it lands. Resolving `fi` in the callback would now select `other`.
    defer_first_menu(cx, &app, window);
    app.update(cx, |app, cx| {
        let panel = app
            .ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .clone();
        panel.update(cx, |panel, _| panel.state.unstaged.swap(0, 1));
    });
    cx.run_until_parked();

    let menu = cx.read(|cx| app.read(cx).file_menu.clone().expect("menu opened"));
    assert_eq!(menu.owner, owner, "deferred menu changed its owner");
    assert_eq!(
        menu.path, frozen,
        "deferred menu re-resolved its row after renumbering"
    );

    // Click the real rendered menu item and execute the real discard path.
    let bounds = draw_menu_and_bounds(cx, window);
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).discard_modal().is_some()),
        "the real menu item did not dispatch Discard"
    );
    app.update(cx, |app, cx| app.start_discard(cx));
    app.update(cx, |app, cx| app.start_discard(cx));
    cx.run_until_parked();

    assert!(
        !repo.join(&frozen).exists(),
        "discard acted on a row other than the frozen path"
    );
    assert!(
        repo.join(&other).exists(),
        "discard removed the row that moved into the frozen index"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_menu_freezes_path");
}

pub fn scenario_file_menu_rejects_stale_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = dirty_fixture();
    let fixture_b = dirty_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);

    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        let owner_b = app.active_session().expect("B owner");
        app.switch_repo(0, cx);
        e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
        (app.active_session().expect("A owner"), owner_b)
    });
    assert_ne!(owner_a, owner_b);
    cx.run_until_parked();

    // Event-source gate: the parent callback lands only after B owns the UI.
    defer_first_menu(cx, &app, window);
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        e2e::open_local_panel_no_inputs(app, repo_b.clone(), cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).file_menu.is_none()),
        "A's deferred menu opened after B became active"
    );

    // Action gate: open A's menu, retain its exact intent, then dispatch it
    // after B owns both the active session and Commit Panel.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
    });
    defer_first_menu(cx, &app, window);
    cx.run_until_parked();
    let stale: FileMenu = cx.read(|cx| app.read(cx).file_menu.clone().expect("A menu"));

    // Switch to B, attach B's real panel, and restore the stale intent as if
    // switch-time cleanup had missed it. The narrow seam invokes the same
    // dispatch helper retained render callbacks call.
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        e2e::open_local_panel_no_inputs(app, repo_b.clone(), cx);
        app.file_menu = Some(stale.clone());
    });
    app.update(cx, |app, cx| {
        e2e::dispatch_file_menu_discard(app, &stale, cx);
    });

    assert_eq!(
        cx.read(|cx| app.read(cx).active_session()),
        Some(owner_b),
        "fixture did not switch to B"
    );
    assert!(
        cx.read(|cx| app.read(cx).discard_modal().is_none()),
        "A's retained menu callback dispatched against B"
    );
    assert!(
        repo_b.join("alpha.txt").exists(),
        "B was mutated by A's menu"
    );
    assert!(
        repo_b.join("zeta.txt").exists(),
        "B was mutated by A's menu"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_menu_rejects_stale_owner");
}
