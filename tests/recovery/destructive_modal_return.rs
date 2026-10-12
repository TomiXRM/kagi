use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::{paint, press_key, wait_idle};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
#[path = "../support/git_fixture.rs"]
mod git_fixture;

pub(crate) fn focused_confirm(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
) {
    focused_activation(cx, app, window, id, "enter", "tab");
}

fn focused_activation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
    key: &str,
    direction: &str,
) {
    press_key(cx, app, window, "f19");
    for _ in 0..80 {
        kagi::ui::button_style::modal_focus_probe();
        cx.simulate_keystrokes(window, direction);
        cx.run_until_parked();
        paint(cx, window);
        cx.simulate_keystrokes(window, "f19");
        if kagi::ui::button_style::modal_focus_probe() == Some(id) {
            crate::keyboard_nav::keys(cx, window, key);
            cx.run_until_parked();
            return;
        }
    }
    panic!("{id} is not reachable through {direction}");
}

pub fn scenario_dirty_guard_focus_trap(cx: &mut VisualTestAppContext) {
    use gpui::Focusable;
    use std::time::{Duration, Instant};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for lang in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
        let fixture = build_fixture();
        let repo = fixture.path();
        let before = std::fs::read(repo.join("README.md")).unwrap();
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_editor_workspace(cx);
        });
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .unwrap();
        editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.run_until_parked();
            paint(cx, window);
            if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
                break;
            }
            assert!(Instant::now() < deadline, "editor buffer did not load");
            std::thread::sleep(Duration::from_millis(2));
        }
        let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.draw(cx).clear();
        })
        .unwrap();
        cx.simulate_keystrokes(window, "x");
        cx.run_until_parked();
        assert!(cx.read(|cx| editor.read(cx).dirty));
        for cancel_key in ["escape", "enter"] {
            app.update(cx, |app, cx| app.show_graph_mode(cx));
            cx.run_until_parked();
            paint(cx, window);
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()));
            for direction in ["tab", "shift-tab"] {
                let mut seen = std::collections::HashSet::new();
                for stop in 0..10 {
                    kagi::ui::button_style::modal_focus_probe();
                    cx.simulate_keystrokes(window, direction);
                    cx.run_until_parked();
                    paint(cx, window);
                    cx.simulate_keystrokes(window, "f19");
                    let focused = kagi::ui::button_style::modal_focus_probe();
                    assert!(
                        matches!(focused, Some("editor-dirty-guard-cancel" | "editor-dirty-guard-discard")),
                        "{lang:?}: {direction} stop {stop}: dirty guard focus escaped its controls: {focused:?}"
                    );
                    cx.update_window(window, |_, window, cx| {
                        assert!(!input.read(cx).focus_handle(cx).contains_focused(window, cx));
                    })
                    .unwrap();
                    seen.insert(focused.unwrap());
                }
                assert_eq!(
                    seen.len(),
                    2,
                    "both dialog controls must participate in the cycle"
                );
            }
            press_key(cx, &app, window, cancel_key);
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
            assert!(cx.read(|cx| editor.read(cx).dirty));
            assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
        }
        drop(input);
        drop(editor);
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS dirty_guard_focus_trap");
}

pub fn scenario_destructive_modal_return(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for lang in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
        let fixture = build_fixture();
        let repo = fixture.path();
        let path = repo.join("README.md");
        std::fs::write(&path, "# changed\n").unwrap();
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        app.update(cx, |app, cx| {
            kagi::ui::e2e::open_local_panel_no_inputs(app, repo.to_path_buf(), cx)
        });
        cx.run_until_parked();
        let owner = cx.read(|cx| {
            app.read(cx)
                .ui()
                .commit_panel
                .as_ref()
                .unwrap()
                .read(cx)
                .owner
        });
        app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
        wait_idle(cx, &app);
        paint(cx, window);
        let cancel = kagi::ui::button_style::recorded_modal_button("discard-cancel").unwrap();
        assert_eq!(cancel.role, gpui::Role::Button);
        assert_eq!(cancel.label, kagi_ui_core::i18n::Msg::PlanCancel.t());
        let before = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
        press_key(cx, &app, window, "enter");
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).discard_modal().is_some()) {
            // Before the fix the first implicit default armed; another Return
            // executed the discard without selecting its danger button.
            press_key(cx, &app, window, "enter");
            wait_idle(cx, &app);
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# changed\n",
            "root Return must never discard files"
        );
        assert!(
            cx.read(|cx| app.read(cx).discard_modal().is_none()),
            "Return cancels the destructive modal"
        );
        assert!(kagi_ui_core::klog::tail()
            .iter()
            .any(|line| line == "[kagi] discard: cancelled"));
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            before
        );
        app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
        wait_idle(cx, &app);
        paint(cx, window);
        focused_confirm(cx, &app, window, "discard-confirm");
        assert!(cx.read(|cx| app.read(cx).discard_modal().unwrap().confirm_armed));
        paint(cx, window);
        focused_activation(cx, &app, window, "discard-confirm", "space", "shift-tab");
        wait_idle(cx, &app);
        assert_ne!(std::fs::read_to_string(&path).unwrap(), "# changed\n");
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            before + 1
        );
        cancel_other_danger_modals(cx, &app, window, repo);
        create_branch_with_return(cx, &app, window, repo);
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS destructive_modal_return");
}

fn cancel_other_danger_modals(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    for kind in ["amend", "reset", "force-lease"] {
        let fingerprint = crate::macos::repo_fingerprint(repo);
        let records = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
        app.update(cx, |app, cx| match kind {
            "amend" => app.open_amend_modal_with_message(
                kagi_git::AmendMode::MessageOnly,
                "explicit rewrite".into(),
                cx,
            ),
            "reset" => app.open_reset_current_modal(
                kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD~1"])),
                cx,
            ),
            _ => app.open_force_lease_push_modal(cx),
        });
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).active_modal.is_some()));
        press_key(cx, &app, window, "enter");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).active_modal.is_none()),
            "{kind}: Return cancels"
        );
        assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            records
        );
    }
}

fn create_branch_with_return(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    let head = kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
    paint(cx, window);
    let input = cx.read(|cx| {
        app.read(cx)
            .create_branch_modal()
            .unwrap()
            .input_state
            .clone()
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        use gpui::Focusable;
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "s a f e");
    cx.run_until_parked();
    press_key(cx, &app, window, "enter");
    wait_idle(cx, &app);
    assert!(!git_fixture::git_output(repo, &["branch", "--list", "safe"]).is_empty());
    drop(input);
}
