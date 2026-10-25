use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::{paint, press_key, wait_idle};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
#[path = "../support/git_fixture.rs"]
mod git_fixture;

/// Root Return neither approves nor dismisses the current danger sheet.
pub(crate) fn root_return_keeps_modal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    let kind = cx.read(|cx| std::mem::discriminant(app.read(cx).active_modal.as_ref().unwrap()));
    let fingerprint = crate::macos::repo_fingerprint(repo);
    let leased = cx.read(|cx| app.read(cx).app_sessions.has_leases());
    let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100);
    for _ in 0..2 {
        press_key(cx, app, window, "enter");
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                std::mem::discriminant(state.active_modal.as_ref().expect("Return retains sheet")),
                kind
            );
            assert_eq!(
                state.app_sessions.has_leases(),
                leased,
                "Return cannot change write admission"
            );
        });
        assert_eq!(crate::macos::repo_fingerprint(repo), fingerprint);
        assert_eq!(
            format!("{:?}", kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)),
            format!("{receipts:?}"),
            "Return must not change receipts"
        );
    }
}

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
        editor.update(cx, |view, cx| view.set_markdown_preview(false, cx));
        paint(cx, window);
        let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.draw(cx).clear();
        })
        .unwrap();
        cx.simulate_input(window, "x");
        crate::busy_dismiss::wait_ui(cx, &app, "native typed editor becomes dirty", |_, cx| {
            editor.read(cx).dirty
        });
        assert!(cx.read(|cx| editor.read(cx).dirty));
        for cancel_key in ["escape", "cmd-."] {
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
            root_return_keeps_modal(cx, &app, window, repo);
            assert!(cx.read(|cx| editor.read(cx).dirty));
            press_key(cx, &app, window, cancel_key);
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
            assert!(cx.read(|cx| editor.read(cx).dirty));
            assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
            paint(cx, window);
            let buffer_before = cx.read(|cx| input.read(cx).value().to_string());
            cx.simulate_input(window, "z");
            cx.run_until_parked();
            assert_ne!(
                cx.read(|cx| input.read(cx).value().to_string()),
                buffer_before,
                "{lang:?}: typing after {cancel_key} must return to the Editor Input"
            );
        }
        drop(input);
        drop(editor);
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        cx.run_until_parked();
        paint(cx, window);
        focused_confirm(cx, &app, window, "editor-dirty-guard-discard");
        git_fixture::git(repo, &["branch", "focus-cancel-target"]);
        app.update(cx, |app, cx| {
            app.open_delete_branch_modal("focus-cancel-target", cx)
        });
        cx.run_until_parked();
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_some()));
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_none()));
        assert!(
            !git_fixture::git_output(repo, &["branch", "--list", "focus-cancel-target"]).is_empty()
        );
        let sidebar_before = cx.read(|cx| app.read(cx).sidebar.visible);
        cx.simulate_keystrokes(window, "secondary-b");
        cx.run_until_parked();
        assert_ne!(
            cx.read(|cx| app.read(cx).sidebar.visible),
            sidebar_before,
            "Delete cancellation must restore root shortcut dispatch"
        );
        cx.simulate_keystrokes(window, "secondary-b");
        cx.run_until_parked();
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS dirty_guard_focus_trap");
}

pub fn scenario_alert_focus_reload(cx: &mut VisualTestAppContext) {
    alert_focus_transition(cx, true);
}

pub fn scenario_alert_focus_notice(cx: &mut VisualTestAppContext) {
    alert_focus_transition(cx, false);
}

fn alert_focus_transition(cx: &mut VisualTestAppContext, reload: bool) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for lang in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
        let fixture = build_fixture();
        let repo = fixture.path();
        git_fixture::git(repo, &["branch", "focus-transition-target"]);
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            if reload {
                app.open_amend_modal_with_message(
                    kagi_git::AmendMode::MessageOnly,
                    "focus reload".into(),
                    cx,
                );
            } else {
                app.open_delete_branch_modal("focus-transition-target", cx);
            }
        });
        cx.run_until_parked();
        paint(cx, window);
        assert!(cx.read(|cx| if reload {
            app.read(cx).amend_modal().is_some()
        } else {
            app.read(cx).delete_branch_modal().is_some()
        }));
        if reload {
            app.update(cx, |app, cx| app.reload(cx));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                cx.run_until_parked();
                paint(cx, window);
                if cx.read(|cx| app.read(cx).amend_modal().is_none()) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "reload did not retire alert"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(cx.read(|cx| app.read(cx).amend_modal().is_none()));
            let before = cx.read(|cx| app.read(cx).sidebar.visible);
            cx.simulate_keystrokes(window, "secondary-b");
            cx.run_until_parked();
            assert_ne!(
                cx.read(|cx| app.read(cx).sidebar.visible),
                before,
                "{lang:?}: reload retiring the trap must restore root actions"
            );
            cx.simulate_keystrokes(window, "secondary-b");
            cx.run_until_parked();
        } else {
            app.update(cx, |app, cx| {
                kagi::ui::e2e::deliver_app_notice(app, "Queued behind alert");
                cx.notify();
            });
            cx.simulate_keystrokes(window, "escape");
            cx.run_until_parked();
            paint(cx, window);
            assert_eq!(
                cx.read(|cx| kagi::ui::e2e::app_notice_message(app.read(cx)).map(str::to_owned)),
                Some("Queued behind alert".into())
            );
            cx.simulate_keystrokes(window, "escape");
            cx.run_until_parked();
            paint(cx, window);
            assert!(
                cx.read(|cx| app.read(cx).app_notice().is_none()),
                "{lang:?}: the successor notice must receive Escape"
            );
        }
        assert!(
            !git_fixture::git_output(repo, &["branch", "--list", "focus-transition-target"])
                .is_empty()
        );
        unmount(cx, app, window);
    }
    eprintln!(
        "[gui-e2e] PASS alert_focus_{}",
        if reload { "reload" } else { "notice" }
    );
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
        root_return_keeps_modal(cx, &app, window, repo);
        assert!(!cx.read(|cx| app.read(cx).discard_modal().unwrap().confirm_armed));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# changed\n",
            "root Return must never discard files"
        );
        assert!(
            cx.read(|cx| app.read(cx).discard_modal().is_some()),
            "Return retains the destructive modal"
        );
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            before
        );
        press_key(cx, &app, window, "escape");
        assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
        assert!(kagi_ui_core::klog::tail()
            .iter()
            .any(|line| line == "[kagi] discard: cancelled"));
        app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
        wait_idle(cx, &app);
        paint(cx, window);
        // Without the backup contract, both explicit stages remain mandatory.
        app.update(cx, |app, cx| {
            let mut modal = app.discard_modal().unwrap().clone();
            std::sync::Arc::make_mut(&mut modal.plan).recovery = None;
            app.set_discard_modal(modal);
            cx.notify();
        });
        paint(cx, window);
        focused_confirm(cx, &app, window, "discard-confirm");
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).discard_modal().unwrap().confirm_armed));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# changed\n");
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            before
        );
        let (_, notice) = kagi::ui::dialog_a11y::recorded_note("discard-armed-note").unwrap();
        assert_eq!(notice, kagi_ui_core::i18n::Msg::DiscardArmedNotice.t());
        if lang == kagi_ui_core::i18n::Lang::Ja {
            assert!(notice.contains("バックアップがありません"));
            assert!(!notice.contains("Working-tree changes"));
        }
        root_return_keeps_modal(cx, &app, window, repo);
        assert!(cx.read(|cx| app.read(cx).discard_modal().unwrap().confirm_armed));
        press_key(cx, &app, window, "escape");
        assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# changed\n");
        app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
        wait_idle(cx, &app);
        paint(cx, window);
        assert!(!cx.read(|cx| app.read(cx).discard_modal().unwrap().requires_two_stage()));
        focused_activation(cx, &app, window, "discard-confirm", "space", "shift-tab");
        wait_idle(cx, &app);
        assert_ne!(std::fs::read_to_string(&path).unwrap(), "# changed\n");
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            before + 1
        );
        assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
        let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100);
        let receipt = receipts
            .iter()
            .rev()
            .find(|entry| entry.op == "discard")
            .unwrap();
        assert!(
            !receipt.backup_refs.is_empty(),
            "single confirm retains recovery"
        );
        assert_eq!(
            kagi_git::Backend::open(repo)
                .unwrap()
                .read_backup(&receipt.backup_refs[0])
                .unwrap(),
            b"# changed\n",
        );
        cancel_other_danger_modals(cx, &app, window, repo);
        create_branch_with_return(cx, &app, window, repo);
        unmount(cx, app, window);
        amend_confirmation_policy(cx, lang);
    }
    eprintln!("[gui-e2e] PASS destructive_modal_return");
}

fn amend_confirmation_policy(cx: &mut VisualTestAppContext, lang: kagi_ui_core::i18n::Lang) {
    use kagi_ui_core::i18n::Msg;
    let fixture = build_fixture();
    let repo = fixture.path();
    git_fixture::git(repo, &["checkout", "-q", "-b", "amend-review"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.open_amend_modal_with_message(
            kagi_git::AmendMode::MessageOnly,
            "local replacement".into(),
            cx,
        );
    });
    wait_idle(cx, &app);
    paint(cx, window);
    let old = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    let records = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
    assert!(!cx.read(|cx| app.read(cx).amend_modal().unwrap().requires_two_stage()));
    focused_confirm(cx, &app, window, "amend-confirm");
    wait_idle(cx, &app);
    assert!(cx.read(|cx| app.read(cx).amend_modal().is_none()));
    assert_ne!(git_fixture::git_output(repo, &["rev-parse", "HEAD"]), old);
    assert_eq!(
        git_fixture::git_output(repo, &["rev-parse", "HEAD@{1}"]),
        old
    );
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
        records + 1
    );

    // A real fetched upstream makes the existing plan warn about published history.
    let origin = tempfile::tempdir().unwrap();
    let bare = origin.path().join("origin.git");
    git_fixture::git(
        repo,
        &[
            "clone",
            "-q",
            "--bare",
            repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    git_fixture::git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git_fixture::git(repo, &["fetch", "-q", "origin"]);
    git_fixture::git(repo, &["branch", "--set-upstream-to=origin/amend-review"]);
    let published = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    let records = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
    for cancel in [true, false] {
        app.update(cx, |app, cx| {
            app.open_amend_modal_with_message(
                kagi_git::AmendMode::MessageOnly,
                "published replacement".into(),
                cx,
            );
            cx.notify();
        });
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).amend_modal().unwrap().requires_two_stage()));
        focused_confirm(cx, &app, window, "amend-confirm");
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).amend_modal().unwrap().confirm_armed));
        assert_eq!(
            git_fixture::git_output(repo, &["rev-parse", "HEAD"]),
            published
        );
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            records
        );
        let (_, notice) = kagi::ui::dialog_a11y::recorded_note("amend-armed-note").unwrap();
        assert_eq!(
            notice,
            format!(
                "{}\n{}",
                Msg::AmendArmedNotice.t(),
                Msg::AmendArmedRecovery.t()
            )
        );
        if lang == kagi_ui_core::i18n::Lang::Ja {
            assert!(notice.contains("公開済み"));
            assert!(!notice.contains("This rewrites"));
        }
        if cancel {
            root_return_keeps_modal(cx, &app, window, repo);
            assert!(cx.read(|cx| app.read(cx).amend_modal().unwrap().confirm_armed));
            press_key(cx, &app, window, "escape");
            assert!(cx.read(|cx| app.read(cx).amend_modal().is_none()));
            assert_eq!(
                git_fixture::git_output(repo, &["rev-parse", "HEAD"]),
                published
            );
        } else {
            focused_activation(cx, &app, window, "amend-confirm", "space", "shift-tab");
            wait_idle(cx, &app);
            assert_ne!(
                git_fixture::git_output(repo, &["rev-parse", "HEAD"]),
                published
            );
            assert_eq!(
                git_fixture::git_output(repo, &["rev-parse", "origin/amend-review"]),
                published
            );
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
                records + 1
            );
        }
    }
    unmount(cx, app, window);
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
        root_return_keeps_modal(cx, app, window, repo);
        press_key(cx, app, window, "escape");
        assert!(
            cx.read(|cx| app.read(cx).active_modal.is_none()),
            "{kind}: Escape cancels"
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

pub fn scenario_modal_command_period(cx: &mut VisualTestAppContext) {
    use gpui::Focusable;
    use gpui_component::WindowExt;
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    let (app, window) = crate::input_context_menus::mount_at(cx, repo, 1000.);
    for key in ["cmd-.", "escape"] {
        app.update(cx, |app, cx| app.open_create_branch_modal(head.clone(), cx));
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
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.refresh();
            window.draw(cx).clear();
            assert!(window.has_focused_input(cx));
        })
        .unwrap();
        cx.simulate_keystrokes(window, key);
        cx.run_until_parked();
        paint(cx, window);
        assert!(
            cx.read(|cx| app.read(cx).create_branch_modal().is_none()),
            "{key} must cancel Create Branch with focus in its name field"
        );
        drop(input);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS modal_command_period");
}

pub fn scenario_input_context_menu_locale(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::{i18n::Lang, input};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    let (app, window) = crate::input_context_menus::mount_at(cx, repo, 1000.);
    app.update(cx, |app, cx| {
        app.set_lang(Lang::Ja, cx);
        app.open_create_branch_modal(head, cx);
    });
    paint(cx, window);
    crate::native_ax::activate(cx);
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    input::capture_next_menu_for_e2e();
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), "input-modal-field")
        .expect("Create Branch name field");
    cx.simulate_mouse_down(
        window,
        bounds.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        window,
        bounds.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    let menu =
        input::take_menu_for_e2e().expect("right-click must produce Kagi's localized menu model");
    let labels: Vec<_> = menu
        .items
        .iter()
        .filter_map(|item| match item {
            gpui::MenuItem::Action { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, ["カット", "コピー", "ペースト", "すべてを選択"]);
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS input_context_menu_locale");
}
