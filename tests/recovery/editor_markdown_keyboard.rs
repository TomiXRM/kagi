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
    let original = std::fs::read_to_string(repo.join("README.md")).unwrap();
    std::fs::write(repo.join("z-other.md"), "# Other document\n").unwrap();
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
        editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
        wait_for(cx, &editor, window, loaded);
        assert_local_history_keys(cx, &app, &editor, window);
        let saved_preview = preview_pixels(cx, &editor, window);

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
        verify_mode_button(cx, &editor, window);
        assert_local_history_keys(cx, &app, &editor, window);
        assert!(cx.read(|cx| editor.read(cx).dirty));
        let draft_preview = preview_pixels(cx, &editor, window);
        assert!(
            draft_preview != saved_preview,
            "unsaved Markdown Preview still paints the saved document"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            original
        );
        editor.update(cx, |view, cx| view.open_tab("z-other.md".into(), cx));
        wait_for(cx, &editor, window, loaded);
        editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
        wait_for(cx, &editor, window, loaded);
        assert!(
            preview_pixels(cx, &editor, window) == draft_preview,
            "tab return changed draft Preview"
        );
        editor.update(cx, |view, cx| view.set_markdown_preview(false, cx));
        paint(cx, window);
        cx.update_window(window, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx);
        })
        .unwrap();
        keys(cx, window, "cmd-z");
        assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), original);
        keys(cx, window, "cmd-shift-z");
        assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), edited);
        editor.update(cx, |view, cx| view.set_markdown_preview(true, cx));
        paint(cx, window);
        assert_local_history_keys(cx, &app, &editor, window);
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            original,
            "tab and Source/Preview round trips must not save implicitly"
        );
        keys(cx, window, "cmd-s");
        wait_for(cx, &editor, window, |view| !view.dirty);
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            edited
        );
        assert!(
            preview_pixels(cx, &editor, window) == draft_preview,
            "saving must not be necessary to update the Preview"
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

fn preview_pixels(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) -> Vec<[u8; 4]> {
    paint(cx, window);
    let strip = cx.read(|cx| editor.read(cx).tab_strip_bounds_for_e2e().unwrap());
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native Markdown Preview capture");
        let scale = window.scale_factor();
        let scaled = kagi_ui_core::theme::scaled;
        // Actual center-pane bounds; omit tabs, dirty dots, toolbar and scrollbar.
        let left = ((f32::from(strip.left()) + scaled(16.)) * scale) as u32;
        let right = ((f32::from(strip.right()) - scaled(16.)) * scale) as u32;
        let top = ((f32::from(strip.bottom()) + scaled(40.)) * scale) as u32;
        let bottom = top + (scaled(120.) * scale) as u32;
        let mut pixels = Vec::with_capacity(((right - left) * (bottom - top)) as usize);
        for y in top..bottom {
            for x in left..right {
                pixels.push(image.get_pixel(x, y).0);
            }
        }
        pixels
    })
    .unwrap()
}

fn verify_mode_button(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    use crate::native_ax;
    use kagi_ui_core::i18n::Msg;

    let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
    let draft = cx.read(|cx| input.read(cx).value().to_string());
    native_ax::activate(cx);
    paint(cx, window);
    native_ax::focus_named(cx, window, "Button", Msg::EditorWorkspacePreviewEdit.t());
    keys(cx, window, "shift-tab");
    keys(cx, window, "tab");
    paint(cx, window);
    let identity = format!("{:?}", gpui::ElementId::from("ews-md-toggle"));
    assert_eq!(native_ax::focused(cx, window)["element_id"], identity);
    for (key, previewing) in [
        ("enter", false),
        ("space", true),
        ("space", false),
        ("enter", true),
    ] {
        keys(cx, window, key);
        paint(cx, window);
        assert_eq!(cx.read(|cx| editor.read(cx).preview_markdown), previewing);
        let focused = native_ax::focused(cx, window);
        assert_eq!(focused["element_id"], identity, "mode changes retain focus");
        assert_eq!(focused["aria"]["role"], "Button");
        assert_eq!(
            focused["aria"]["label"],
            if previewing {
                Msg::EditorWorkspacePreviewEdit.t()
            } else {
                Msg::EditorWorkspacePreviewShow.t()
            }
        );
    }
    for (name, previewing) in [
        (Msg::EditorWorkspacePreviewEdit.t(), false),
        (Msg::EditorWorkspacePreviewShow.t(), true),
    ] {
        assert!(native_ax::press(cx, window, name));
        paint(cx, window);
        assert_eq!(cx.read(|cx| editor.read(cx).preview_markdown), previewing);
    }
    cx.read(|cx| {
        let view = editor.read(cx);
        assert!(view.dirty);
        assert_eq!(view.editor.as_ref().unwrap().entity_id(), input.entity_id());
        assert_eq!(input.read(cx).value().to_string(), draft);
    });
}
