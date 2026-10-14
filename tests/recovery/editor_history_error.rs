//! #1515: production History failures, keyboard Retry and stale ownership.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_viewed::paint;
use gpui::{Entity, Task, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{EditorWorkspaceEvent, EditorWorkspaceView, MiddlePaneTab, RightPaneTab};
use std::{cell::RefCell, rc::Rc};

const CAUSE: &str = "history-read-1515-cause";
fn wait(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: gpui::AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx))) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "production Editor read settles"
        );
        crate::macos::drain_native_events();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn retry_by_tab(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: gpui::AnyWindowHandle,
    key: &str,
) {
    editor.update(cx, |view, _| view.history_retry_focused = false);
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |view, cx| {
            view.focus_selector_for_e2e(false, 1, window, cx)
        });
    })
    .unwrap();
    let mut reached = false;
    for _ in 0..100 {
        cx.simulate_keystrokes(window, "tab");
        paint(cx, window);
        cx.simulate_keystrokes(window, "f19");
        if cx.read(|cx| editor.read(cx).history_retry_focused) {
            reached = true;
            break;
        }
    }
    assert!(reached, "real Tab reaches rendered Retry Button");
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    paint(cx, window);
}
fn assert_error(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: gpui::AnyWindowHandle,
) {
    wait(cx, editor, window, |view| !view.history_loading());
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.history_error.as_deref(), Some(CAUSE));
        let text = view.history_presentation.borrow();
        assert!(text.contains(Msg::EditorHistoryFailed.t()) && text.contains(CAUSE));
        assert!(text.contains(Msg::EditorHistoryRetry.t()));
        assert!(!text.contains(Msg::EditorHistoryEmpty.t()));
    });
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let repo = build_fixture();
    std::fs::write(repo.path().join("empty.txt"), "untracked\n").unwrap();
    let history = kagi_git::file_history(&kagi_git::FileHistoryRequest {
        repo_dir: repo.path().into(),
        file_path: "README.md".into(),
        follow_renames: true,
        include_wip: false,
        limit: 500,
    })
    .unwrap();
    let mut empty = history.clone();
    empty.entries.clear();
    for preset in ["apple-light", "apple-dark"] {
        for lang in [Lang::En, Lang::Ja] {
            for zoom in [1., 1.67] {
                let (app, window) = mount(cx, repo.path());
                app.update(cx, |app, cx| {
                    app.set_theme(preset, cx);
                    app.open_editor_workspace(cx);
                });
                i18n::set_lang(lang);
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                let editor = cx
                    .read(|cx| app.read(cx).ui().editor_workspace.clone())
                    .unwrap();
                let requests = Rc::new(RefCell::new(Vec::new()));
                let output = requests.clone();
                let subscription = cx.update(|cx| {
                    cx.subscribe(&editor, move |_, event, _| {
                        if let EditorWorkspaceEvent::HistoryRequested { req, path } = event {
                            output.borrow_mut().push((*req, path.clone()));
                        }
                    })
                });
                editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
                wait(cx, &editor, window, |view| {
                    !view.file_loading && view.editor.is_some()
                });
                e2e::queue_editor_history(Task::ready(Err(CAUSE.into())));
                cx.update_window(window, |_, window, cx| {
                    editor.update(cx, |view, cx| {
                        view.set_right_tab(RightPaneTab::History, window, cx)
                    })
                })
                .unwrap();
                assert_error(cx, &editor, window);
                let first = requests.borrow()[0].clone();
                cx.update_window(window, |_, window, cx| {
                    editor.update(cx, |view, cx| {
                        view.set_right_tab(RightPaneTab::Diff, window, cx);
                        view.set_middle_tab(MiddlePaneTab::Snapshot, cx);
                        view.set_middle_tab(MiddlePaneTab::Diff, cx);
                        view.set_right_tab(RightPaneTab::History, window, cx);
                    })
                })
                .unwrap();
                assert_error(cx, &editor, window);
                assert_eq!(
                    requests.borrow().len(),
                    1,
                    "mode changes retain failed read owner"
                );
                for key in ["enter", "space"] {
                    let before = requests.borrow().len();
                    e2e::queue_editor_history(Task::ready(Err(CAUSE.into())));
                    retry_by_tab(cx, &editor, window, key);
                    assert_error(cx, &editor, window);
                    assert_eq!(
                        requests.borrow().len(),
                        before + 1,
                        "one Retry dispatch per key"
                    );
                    assert_eq!(
                        requests.borrow().last(),
                        Some(&first),
                        "Retry reuses file/request identity"
                    );
                }
                e2e::queue_editor_history(Task::ready(Ok(history.clone())));
                retry_by_tab(cx, &editor, window, "enter");
                wait(cx, &editor, window, |view| {
                    view.history.is_some() && !view.history_loading()
                });
                cx.read(|cx| {
                    let view = editor.read(cx);
                    assert!(view.history_error.is_none());
                    assert_eq!(
                        view.history.as_ref().unwrap().entries.len(),
                        history.entries.len()
                    );
                    assert!(view.history_presentation.borrow().is_empty());
                });
                let hash = history.entries[0]
                    .commit
                    .as_ref()
                    .unwrap()
                    .full_hash
                    .clone();
                cx.update_window(window, |_, window, cx| {
                    editor.update(cx, |view, cx| {
                        view.select_history_commit(hash.clone(), window, cx);
                        view.set_middle_tab(MiddlePaneTab::Snapshot, cx);
                    })
                })
                .unwrap();
                wait(cx, &editor, window, |view| {
                    !view.snapshot_loading() && view.snapshot.is_some()
                });
                cx.update_window(window, |_, window, cx| {
                    editor.update(cx, |view, cx| {
                        view.set_middle_tab(MiddlePaneTab::Diff, cx);
                        view.set_right_tab(RightPaneTab::Diff, window, cx);
                        view.set_right_tab(RightPaneTab::History, window, cx);
                    })
                })
                .unwrap();
                assert_eq!(
                    cx.read(|cx| editor.read(cx).selected_history_commit.clone()),
                    Some(hash)
                );
                // Hold A's production read, publish B's successful empty read, then reject A's Err.
                e2e::queue_editor_history(Task::ready(Ok(empty.clone())));
                editor.update(cx, |view, cx| view.open_tab("empty.txt".into(), cx));
                wait(cx, &editor, window, |view| {
                    !view.history_loading() && view.history.is_some()
                });
                let (task, reply) = deferred(cx);
                e2e::queue_editor_history(task);
                editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
                e2e::queue_editor_history(Task::ready(Ok(empty.clone())));
                editor.update(cx, |view, cx| view.open_tab("empty.txt".into(), cx));
                wait(cx, &editor, window, |view| {
                    !view.history_loading() && view.history.is_some()
                });
                reply.send(Err("stale-A-1515".into()));
                paint(cx, window);
                cx.read(|cx| {
                    let view = editor.read(cx);
                    assert_eq!(
                        view.open_path.as_deref(),
                        Some(std::path::Path::new("empty.txt"))
                    );
                    assert!(view.history_error.is_none());
                    assert_eq!(
                        *view.history_presentation.borrow(),
                        Msg::EditorHistoryEmpty.t()
                    );
                });
                drop(subscription);
                drop(editor);
                unmount(cx, app, window);
            }
        }
    }
}
