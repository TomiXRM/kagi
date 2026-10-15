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
