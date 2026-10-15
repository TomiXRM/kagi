//! #1603: hidden native host destruction must protect every retained buffer.
use crate::macos::{drain_native_events, git, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    editor_workspace::EditorWorkspaceView,
    i18n::{self, Lang, Msg},
    KagiApp,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

fn fixture(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for file in ["f.txt", "g.txt"] {
        std::fs::write(repo.join(file), "original\n").unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);
    repo.canonicalize().unwrap()
}

fn mount(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    width: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).unwrap();
    let output = Rc::new(RefCell::new(None));
    let captured = output.clone();
    let window = crate::macos::open_offscreen(cx, size(px(width), px(720.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &captured)
    })
    .into();
    let app = output.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window)
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn edit(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    file: &str,
    text: &str,
) -> Entity<EditorWorkspaceView> {
    app.update(cx, |app, cx| {
        if app.ui().editor_workspace.is_none() {
            app.open_editor_workspace(cx);
        }
    });
    let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().unwrap());
    editor.update(cx, |editor, cx| editor.activate_tab(file.into(), cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        draw(cx, window);
        if cx.read(|cx| editor.read(cx).editor.is_some() && !editor.read(cx).file_loading) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "editor load timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let input = cx.read(|cx| {
        editor
            .read(cx)
            .editor
            .clone()
            .expect("loaded native editor Input")
    });
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(text.to_owned(), window, cx));
        editor.update(cx, |editor, cx| {
            editor.dirty = true;
            cx.notify();
        });
    })
    .unwrap();
    cx.run_until_parked();
    editor
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    Quit,
    Close,
    Native,
    Settings,
}

fn request(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    entry: Entry,
) {
    if matches!(entry, Entry::Native) {
        use objc2::{msg_send, rc::Retained, runtime::AnyObject};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let native = cx
            .update_window(window, |_, window, _| {
                let handle = HasWindowHandle::window_handle(window).unwrap();
                let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                    panic!("AppKit window");
                };
                // Retain the live main-thread NSWindow, then release GPUI's window borrow
                // before AppKit invokes the production should-close callback.
                unsafe {
                    let view = handle.ns_view.as_ptr().cast::<AnyObject>();
                    let native: *mut AnyObject = msg_send![view, window];
                    Retained::retain(native).unwrap()
                }
            })
            .unwrap();
        unsafe {
            let _: () = msg_send![&*native, performClose: std::ptr::null::<AnyObject>()];
        }
    } else if matches!(entry, Entry::Settings) {
        let _host = crate::settings_window::SettingsHostGuard::native(cx);
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            })
        })
        .unwrap();
        cx.run_until_parked();
        let settings = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|handle| handle.window_id() != window.window_id())
                .unwrap()
        });
        draw(cx, settings);
        cx.simulate_keystrokes(settings, "cmd-q");
    } else {
        let command = if matches!(entry, Entry::Quit) {
            "app.quit"
        } else {
            "window.close"
        };
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.handle_menu_command(command, window, cx))
        })
        .unwrap();
    }
    cx.run_until_parked();
}

fn alive(cx: &VisualTestAppContext, window: AnyWindowHandle) -> bool {
    cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|handle| handle.window_id() == window.window_id())
    })
}

fn press(cx: &mut VisualTestAppContext, window: AnyWindowHandle, label: &str) {
    crate::native_ax::activate(cx);
    draw(cx, window);
    assert!(
        crate::native_ax::press(cx, window, label),
        "native Press {label}"
    );
    cx.run_until_parked();
}

fn cleanup(cx: &mut VisualTestAppContext, app: Entity<KagiApp>, window: AnyWindowHandle) {
    for handle in cx.read(|cx| cx.windows()) {
        if handle.window_id() != window.window_id() {
            cx.update_window(handle, |_, window, _| window.remove_window())
                .unwrap();
        }
    }
    if alive(cx, window) {
        unmount(cx, app, window);
    } else {
        drop(app);
        cx.update(|_| {});
        cx.run_until_parked();
        drain_native_events();
    }
}

pub fn scenario_host_dirty_guard(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_window_frame"]);
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for (lang, width) in [(Lang::En, 1000.), (Lang::Ja, 940.)] {
        i18n::set_lang(lang);
        for entry in [Entry::Quit, Entry::Close, Entry::Native, Entry::Settings] {
            for background in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let repo = fixture(dir.path(), "a");
                let (app, window) = mount(cx, &repo, width);
                let editor = edit(cx, &app, window, "f.txt", "unsaved first\n");
                if background {
                    drop(edit(cx, &app, window, "g.txt", "unsaved second\n"));
                    let second = fixture(dir.path(), "b");
                    app.update(cx, |app, cx| {
                        assert!(app.open_repository(second, cx));
                    });
                    cx.run_until_parked();
                }
                cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
                request(cx, &app, window, entry);
                cx.read(|cx| {
                    assert!(
                        app.read(cx).editor_dirty_guard_modal().is_some(),
                        "dirty app.quit must show the unsaved-changes guard"
                    );
                    assert!(
                        !cx.global::<e2e::QuitForE2e>().requested,
                        "must not quit before approval: {entry:?}"
                    );
                });
                assert!(alive(cx, window), "dirty {entry:?} must retain the window");
                press(cx, window, Msg::EditorWorkspaceCancel.t());
                assert!(cx.read(|cx| editor.read(cx).any_dirty()));
                cx.read(|cx| {
                    let input = editor.read(cx).editor.as_ref().unwrap();
                    assert_eq!(
                        input.read(cx).value().as_str(),
                        if background {
                            "unsaved second\n"
                        } else {
                            "unsaved first\n"
                        },
                        "Cancel retains the exact edited text"
                    );
                });
                assert!(alive(cx, window));
                assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
                request(cx, &app, window, entry);
                press(cx, window, Msg::EditorWorkspaceDiscard.t());
                if matches!(entry, Entry::Quit | Entry::Settings) {
                    assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
                } else {
                    assert!(!alive(cx, window), "Discard completes {entry:?}");
                }
                assert_eq!(
                    std::fs::read_to_string(repo.join("f.txt")).unwrap(),
                    "original\n"
                );
                drop(editor);
                cleanup(cx, app, window);
            }
            // Clean editors retain the immediate exit path.
            let dir = tempfile::tempdir().unwrap();
            let repo = fixture(dir.path(), "clean");
            let (app, window) = mount(cx, &repo, width);
            app.update(cx, |app, cx| app.open_editor_workspace(cx));
            cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
            request(cx, &app, window, entry);
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
            if matches!(entry, Entry::Quit | Entry::Settings) {
                assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
            } else {
                assert!(!alive(cx, window));
            }
            cleanup(cx, app, window);
        }
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard");
}

pub fn scenario_host_dirty_guard_save(cx: &mut VisualTestAppContext) {
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for conflict in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = fixture(dir.path(), "a");
        let repo_b = fixture(dir.path(), "b");
        let (app, window) = mount(cx, &repo_a, 1000.);
        let editor_a = edit(cx, &app, window, "f.txt", "saved A first\n");
        drop(edit(cx, &app, window, "g.txt", "saved A second\n"));
        app.update(cx, |app, cx| {
            assert!(app.open_repository(repo_b.clone(), cx));
        });
        cx.run_until_parked();
        let editor_b = edit(cx, &app, window, "f.txt", "saved B\n");
        if conflict {
            std::fs::write(repo_a.join("g.txt"), "external change\n").unwrap();
        }
        cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
        request(cx, &app, window, Entry::Quit);
        press(cx, window, Msg::EditorHostSaveAll.t());
        if conflict {
            assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
            assert!(alive(cx, window));
            assert!(cx.read(|cx| editor_a.read(cx).any_dirty()));
            assert_eq!(
                std::fs::read_to_string(repo_a.join("g.txt")).unwrap(),
                "external change\n"
            );
            press(cx, window, Msg::EditorWorkspaceCancel.t());
        } else {
            assert!(
                cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested),
                "Save All quits only after every write succeeded"
            );
            assert_eq!(
                std::fs::read_to_string(repo_a.join("f.txt")).unwrap(),
                "saved A first\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo_a.join("g.txt")).unwrap(),
                "saved A second\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo_b.join("f.txt")).unwrap(),
                "saved B\n"
            );
            assert!(!cx.read(|cx| editor_a.read(cx).any_dirty() || editor_b.read(cx).any_dirty()));
        }
        drop((editor_a, editor_b));
        cleanup(cx, app, window);
    }
    // A write acquired after the confirmation opened must still veto Discard.
    for entry in [Entry::Quit, Entry::Close, Entry::Native] {
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), "held");
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = edit(cx, &app, window, "f.txt", "retained during write\n");
        cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
        request(cx, &app, window, entry);
        let lease = app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap());
        press(cx, window, Msg::EditorWorkspaceDiscard.t());
        assert!(alive(cx, window));
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert!(cx.read(|cx| editor.read(cx).any_dirty()));
        lease.complete();
        drop(editor);
        cleanup(cx, app, window);
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard_save");
}

fn assert_failure_visible(cx: &VisualTestAppContext, app: &Entity<KagiApp>, expected: &str) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Failed(text)
            if text.as_ref() == expected),
            "Save All must explain recovery in the footer"
        );
        assert!(
            app.toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| toast.kind == kagi::ui::ToastKind::Error
                    && toast.message.as_ref() == expected),
            "Save All failure must also surface a bounded toast"
        );
        assert!(app.editor_dirty_guard_modal().unwrap().saving.is_none());
    });
}

pub fn scenario_host_dirty_guard_save_waiting(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), "waiting");
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = edit(cx, &app, window, "f.txt", "retry after writer\n");
        request(cx, &app, window, Entry::Quit);
        let lease = app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap());
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert_failure_visible(cx, &app, Msg::EditorHostSaveWaiting.t());
        assert!(alive(cx, window));
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert!(cx.read(|cx| editor.read(cx).any_dirty()));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "original\n"
        );
        lease.complete();
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert!(!cx.read(|cx| editor.read(cx).any_dirty()));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "retry after writer\n"
        );
        drop(editor);
        cleanup(cx, app, window);
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard_save_waiting");
}

pub fn scenario_host_dirty_guard_save_unwind(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), "unwind");
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = edit(cx, &app, window, "f.txt", "retry after unwind\n");
        request(cx, &app, window, Entry::Quit);
        KagiApp::panic_next_editor_save_for_e2e();
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert_failure_visible(cx, &app, Msg::EditorWorkspaceSaveInterrupted.t());
        assert!(alive(cx, window));
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert!(
            cx.read(|cx| app.read(cx).app_sessions.has_leases()),
            "an interrupted writer retains its recovery hold"
        );
        assert!(cx.read(|cx| editor.read(cx).any_dirty()));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "original\n"
        );
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert_failure_visible(cx, &app, Msg::EditorHostSaveWaiting.t());
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        // Busy presentation must not trap Cancel. Resolve the existing
        // recovery requirement before retrying; an unwind is not permission
        // to release writer admission or assume that no write occurred.
        press(cx, window, Msg::EditorWorkspaceCancel.t());
        app.update(cx, |app, cx| {
            e2e::present_app_notice(app);
            let ids = app.app_sessions.reconcile_ids();
            assert_eq!(ids.len(), 1);
            let read = kagi::app::read_reconcile(&app.app_sessions, ids[0]).unwrap();
            assert!(read.stop_proven() && read.resolved());
            kagi::app::acknowledge(&mut app.app_sessions, read).unwrap();
            app.clear_app_notice();
            cx.notify();
        });
        request(cx, &app, window, Entry::Quit);
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "retry after unwind\n"
        );
        drop(editor);
        cleanup(cx, app, window);
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard_save_unwind");
}

pub fn scenario_host_dirty_guard_close_then_quit(cx: &mut VisualTestAppContext) {
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for save in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), "upgrade");
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = edit(cx, &app, window, "f.txt", "upgraded quit\n");
        request(cx, &app, window, Entry::Close);
        draw(cx, window);
        cx.simulate_keystrokes(window, "cmd-q");
        cx.run_until_parked();
        assert!(alive(cx, window));
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        press(
            cx,
            window,
            if save {
                Msg::EditorHostSaveAll.t()
            } else {
                Msg::EditorWorkspaceDiscard.t()
            },
        );
        assert!(
            cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested),
            "Cmd-Q upgrades the existing CloseWindow guard to application Quit"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            if save {
                "upgraded quit\n"
            } else {
                "original\n"
            }
        );
        drop(editor);
        cleanup(cx, app, window);
        cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard_close_then_quit");
}

pub fn scenario_host_dirty_guard_relaunch(cx: &mut VisualTestAppContext) {
    cx.update(|cx| {
        cx.set_global(e2e::RelaunchForE2e::default());
        cx.set_global(e2e::QuitForE2e::default());
    });
    // Clean completion, Save All, Discard and Cancel all enter the production
    // update-relaunch boundary; only process spawning is captured by the harness.
    for choice in ["clean", "save", "discard", "cancel"] {
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), choice);
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = (choice != "clean")
            .then(|| edit(cx, &app, window, "f.txt", "updated application buffer\n"));
        cx.update(|cx| cx.global_mut::<e2e::RelaunchForE2e>().requested = false);
        app.update(cx, |app, cx| {
            app.request_update_relaunch_for_e2e(
                kagi::update::Relaunch {
                    program: "kagi".into(),
                    args: Vec::new(),
                },
                cx,
            );
        });
        cx.run_until_parked();
        if choice == "clean" {
            assert!(cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested));
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
        } else {
            assert!(
                !cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested),
                "dirty relaunch must wait for explicit approval"
            );
            assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()));
            assert!(alive(cx, window));
            press(
                cx,
                window,
                match choice {
                    "save" => Msg::EditorHostSaveAll.t(),
                    "discard" => Msg::EditorWorkspaceDiscard.t(),
                    _ => Msg::EditorWorkspaceCancel.t(),
                },
            );
            assert_eq!(
                cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested),
                choice != "cancel"
            );
            if choice == "cancel" {
                assert!(alive(cx, window));
                assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
                let editor = editor.as_ref().unwrap();
                cx.read(|cx| {
                    assert!(editor.read(cx).any_dirty());
                    assert_eq!(
                        editor
                            .read(cx)
                            .editor
                            .as_ref()
                            .unwrap()
                            .read(cx)
                            .value()
                            .as_str(),
                        "updated application buffer\n"
                    );
                });
            }
        }
        assert!(
            !cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested),
            "update relaunch must preserve its continuation, not become ordinary Quit"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            if choice == "save" {
                "updated application buffer\n"
            } else {
                "original\n"
            }
        );
        drop(editor);
        cleanup(cx, app, window);
    }
    // A live writer retains even a clean relaunch continuation. A dirty one
    // additionally keeps its exact text until Save All can obtain admission.
    for dirty in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = fixture(dir.path(), "busy-relaunch");
        let (app, window) = mount(cx, &repo, 1000.);
        let editor = dirty.then(|| edit(cx, &app, window, "f.txt", "busy relaunch buffer\n"));
        let lease = app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap());
        cx.update(|cx| cx.global_mut::<e2e::RelaunchForE2e>().requested = false);
        app.update(cx, |app, cx| {
            app.request_update_relaunch_for_e2e(
                kagi::update::Relaunch {
                    program: "kagi".into(),
                    args: Vec::new(),
                },
                cx,
            );
        });
        cx.run_until_parked();
        assert!(alive(cx, window));
        assert!(!cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested));
        assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()));
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert_failure_visible(cx, &app, Msg::EditorHostSaveWaiting.t());
        assert!(!cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "original\n"
        );
        lease.complete();
        press(cx, window, Msg::EditorHostSaveAll.t());
        assert!(cx.read(|cx| cx.global::<e2e::RelaunchForE2e>().requested));
        assert!(!cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested));
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            if dirty {
                "busy relaunch buffer\n"
            } else {
                "original\n"
            }
        );
        if let Some(editor) = editor.as_ref() {
            assert!(!cx.read(|cx| editor.read(cx).any_dirty()));
        }
        drop(editor);
        cleanup(cx, app, window);
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::RelaunchForE2e>();
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_dirty_guard_relaunch");
}
