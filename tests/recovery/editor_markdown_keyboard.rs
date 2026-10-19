//! Markdown Preview owns document shortcuts without disabling dirty-buffer Save.
use super::{loaded, wait_for};
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_viewed::{paint, rev};
use gpui::{AnyWindowHandle, Entity, Focusable as _, VisualTestAppContext};
use kagi::ui::{editor_workspace::EditorWorkspaceView, KagiApp};
use kagi_git::{CommitId, OperationKind};

pub(super) fn verify(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = rev(repo, "HEAD");
    let parent = rev(repo, "HEAD~1");
    let (app, window) = mount(cx, repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| {
            app.open_editor_workspace(cx);
            for (kind, from, to) in [
                (OperationKind::Commit, parent.clone(), head.clone()),
                (OperationKind::UndoCommit, head.clone(), parent.clone()),
            ] {
                app.record_history(
                    kind,
                    "main",
                    CommitId(from),
                    CommitId(to),
                    "Markdown Preview must not invoke Git history",
                );
            }
            let ui = app.ui_mut().unwrap();
            ui.operation_history.undo().unwrap();
            ui.history_seed_attempted = true;
        });
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .unwrap();
        wait_for(cx, &editor, window, loaded);
        assert_local_history_keys(cx, &app, &editor, window);

        editor.update(cx, |view, cx| view.set_markdown_preview(false, cx));
        paint(cx, window);
        let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx);
        })
        .unwrap();
        cx.simulate_input(window, "# Save from Preview\n");
        wait_for(cx, &editor, window, |view| view.dirty);
        let edited = cx.read(|cx| input.read(cx).value().to_string());
        editor.update(cx, |view, cx| view.set_markdown_preview(true, cx));
        paint(cx, window);
        assert_local_history_keys(cx, &app, &editor, window);
        assert!(cx.read(|cx| editor.read(cx).dirty));
        keys(cx, window, "cmd-s");
        wait_for(cx, &editor, window, |view| !view.dirty);
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            edited
        );
        assert_eq!(rev(repo, "HEAD"), head);
        cx.read(|cx| {
            assert!(app.read(cx).active_modal.is_none());
            assert_eq!(app.read(cx).ui().operation_history.cursor(), 1);
        });
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn assert_local_history_keys(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        editor.read(cx).root_focus.clone().focus(window, cx);
    })
    .unwrap();
    for key in ["cmd-z", "cmd-shift-z"] {
        cx.read(|cx| {
            let history = &app.read(cx).ui().operation_history;
            assert!(history.can_undo() && history.can_redo());
            assert!(editor.read(cx).preview_markdown);
        });
        keys(cx, window, key);
        paint(cx, window);
        cx.read(|cx| {
            assert!(
                app.read(cx).active_modal.is_none(),
                "Markdown Preview opened a Git history modal: {key}"
            );
            assert_eq!(app.read(cx).ui().operation_history.cursor(), 1);
        });
    }
}
