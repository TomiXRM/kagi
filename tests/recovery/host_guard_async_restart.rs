//! #1614: a hidden install completing cannot take over a newer user decision.
use crate::{
    evidence_support::deferred,
    host_dirty_guard::{alive, cleanup, draw, edit, fixture, mount, press},
};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{
    button_style, e2e,
    i18n::{self, Lang, Msg},
    modals::{EditorFsPromptKind, EditorPendingIntent},
    KagiApp,
};

fn assert_restart_retained(cx: &VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.update_installing, "successful installer must settle");
        let relaunch = app
            .update_pending_relaunch
            .as_ref()
            .expect("installed update remains available for explicit Restart Kagi");
        assert_eq!(relaunch.program, "installed-kagi");
        assert_eq!(relaunch.args, ["--retained-update"]);
        assert_eq!(
            app.update_status.as_deref(),
            Some(Msg::UpdateRestartReady.t())
        );
        assert!(!cx.global::<e2e::RelaunchForE2e>().requested);
        assert!(!cx.global::<e2e::QuitForE2e>().requested);
    });
}

pub fn scenario_host_guard_async_update_preserves_modal(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["update_skipped"]);
    cx.update(|cx| {
        cx.set_global(e2e::RelaunchForE2e::default());
        cx.set_global(e2e::QuitForE2e::default());
    });
    for (language, width) in [(Lang::En, 1000.), (Lang::Ja, 940.)] {
        i18n::set_lang(language);
        for case in ["close-tab", "rename-dirty", "rename-clean-busy"] {
            let dir = tempfile::tempdir().unwrap();
            let repo = fixture(dir.path(), case);
            let (app, window) = mount(cx, &repo, width);
            let dirty = case != "rename-clean-busy";
            let editor =
                dirty.then(|| edit(cx, &app, window, "f.txt", "retained editor draft 日本語\n"));
            let editor_input = cx.read(|cx| {
                editor
                    .as_ref()
                    .map(|editor| editor.read(cx).editor.clone().unwrap())
            });
            let lease = (!dirty)
                .then(|| app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap()));
            cx.update(|cx| {
                cx.global_mut::<e2e::RelaunchForE2e>().requested = false;
                cx.global_mut::<e2e::QuitForE2e>().requested = false;
            });
            app.update(cx, |app, cx| {
                app.update_checked = true;
                app.update_available = Some(crate::recovery_operations::fake_update_offer());
                app.open_update_modal();
                cx.notify();
            });
            let (install, reply) = deferred(cx);
            e2e::queue_update_install(install);
            press(cx, window, Msg::UpdateNow.t());
            assert!(
                !e2e::update_install_queued(),
                "real Update now entry consumes installer"
            );
            assert!(cx.read(|cx| app.read(cx).update_installing));
            press(cx, window, Msg::UpdateHideContinuing.t());
            assert!(cx.read(|cx| app.read(cx).update_modal().is_none()));
            assert!(cx.read(|cx| app.read(cx).update_installing));

            // Open an independent decision *after* hiding installation. These are
            // ordinary editor entry points, not an injected relaunch continuation.
            let rename_input = if case == "close-tab" {
                editor.as_ref().unwrap().update(cx, |editor, cx| {
                    editor.request_close_tab("f.txt".into(), cx);
                });
                cx.run_until_parked();
                cx.read(|cx| {
                    let modal = app.read(cx).editor_dirty_guard_modal().unwrap();
                    assert!(matches!(&modal.intent, EditorPendingIntent::CloseTab(path) if path == std::path::Path::new("f.txt")));
                });
                None
            } else {
                app.update(cx, |app, cx| {
                    app.open_editor_fs_prompt(
                        EditorFsPromptKind::Rename,
                        "g.txt".into(),
                        "g.txt".into(),
                        cx,
                    );
                });
                draw(cx, window);
                let input = cx.read(|cx| {
                    app.read(cx)
                        .editor_fs_prompt_modal()
                        .unwrap()
                        .input_state
                        .clone()
                        .unwrap()
                });
                cx.update_window(window, |_, window, cx| {
                    input.update(cx, |input, cx| {
                        input.set_value("renamed 日本語.txt", window, cx);
                    });
                })
                .unwrap();
                cx.run_until_parked();
                Some(input)
            };
            draw(cx, window);
            reply.send(Ok(kagi::update::Relaunch {
                program: "installed-kagi".into(),
                args: vec!["--retained-update".into()],
            }));
            cx.run_until_parked();
            draw(cx, window);

            // FAIL-before: successful completion used to replace this modal with
            // Relaunch (or exit a clean host), silently discarding the user's intent.
            cx.read(|cx| {
                let app = app.read(cx);
                if let Some(input) = rename_input.as_ref() {
                    let modal = app.editor_fs_prompt_modal().expect(
                        "background update success must preserve the user's rename prompt",
                    );
                    assert_eq!(modal.kind, EditorFsPromptKind::Rename);
                    assert_eq!(modal.base, std::path::Path::new("g.txt"));
                    assert_eq!(modal.input, "renamed 日本語.txt");
                    assert_eq!(modal.input_state.as_ref().unwrap().entity_id(), input.entity_id());
                    assert_eq!(input.read(cx).value().as_str(), "renamed 日本語.txt");
                    assert!(modal.error.is_none());
                } else {
                    let modal = app.editor_dirty_guard_modal().expect(
                        "background update success must preserve the user's Close Tab guard",
                    );
                    assert!(matches!(&modal.intent, EditorPendingIntent::CloseTab(path) if path == std::path::Path::new("f.txt")),
                        "the unrelated Close Tab decision must not become Relaunch");
                    assert!(modal.saving.is_none());
                }
                if let Some(editor) = editor.as_ref() {
                    let editor = editor.read(cx);
                    assert!(editor.any_dirty());
                    assert_eq!(editor.editor.as_ref().unwrap().entity_id(), editor_input.as_ref().unwrap().entity_id());
                    assert_eq!(editor_input.as_ref().unwrap().read(cx).value().as_str(), "retained editor draft 日本語\n");
                }
                assert!(app.update_modal().is_none());
            });
            assert_restart_retained(cx, &app);
            assert!(alive(cx, window));
            assert_eq!(
                std::fs::read_to_string(repo.join("f.txt")).unwrap(),
                "original\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo.join("g.txt")).unwrap(),
                "original\n"
            );
            assert!(!repo.join("renamed 日本語.txt").exists());

            // Neither lease settlement nor cancelling the independent modal is
            // approval to restart. The installed continuation must survive both.
            if let Some(lease) = lease {
                lease.complete();
                cx.run_until_parked();
                draw(cx, window);
                assert!(cx.read(|cx| app.read(cx).editor_fs_prompt_modal().is_some()));
                assert_restart_retained(cx, &app);
            }
            press(cx, window, Msg::EditorWorkspaceCancel.t());
            assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));
            draw(cx, window);
            assert_restart_retained(cx, &app);
            if let Some(editor) = editor.as_ref() {
                assert!(cx.read(|cx| editor.read(cx).any_dirty()));
            }
            app.update(cx, |app, cx| {
                app.open_update_modal();
                cx.notify();
            });
            draw(cx, window);
            let restart = button_style::recorded_modal_button("update-now").unwrap();
            assert_eq!(restart.label, Msg::UpdateRestart.t());
            assert!(!restart.disabled);
            assert!(e2e::control_bounds(window.window_id(), "update/status").is_some());
            assert_restart_retained(cx, &app);
            press(cx, window, Msg::UpdateRestart.t());
            if dirty {
                cx.read(|cx| {
                    assert!(matches!(
                        app.read(cx).editor_dirty_guard_modal().unwrap().intent,
                        EditorPendingIntent::Relaunch
                    ));
                    assert!(!cx.global::<e2e::RelaunchForE2e>().requested);
                });
                press(cx, window, Msg::EditorWorkspaceDiscard.t());
            }
            cx.read(|cx| {
                assert!(cx.global::<e2e::RelaunchForE2e>().requested);
                assert!(!cx.global::<e2e::QuitForE2e>().requested);
                assert!(app.read(cx).update_pending_relaunch.is_none());
            });
            assert_eq!(
                std::fs::read_to_string(repo.join("f.txt")).unwrap(),
                "original\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo.join("g.txt")).unwrap(),
                "original\n"
            );
            assert!(!repo.join("renamed 日本語.txt").exists());
            drop(rename_input);
            drop(editor_input);
            drop(editor);
            cleanup(cx, app, window);
        }
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::RelaunchForE2e>();
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_guard_async_update_preserves_modal");
}
