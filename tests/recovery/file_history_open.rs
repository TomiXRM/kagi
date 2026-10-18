//! #1308: Open File opens the exact selected path, or keeps History with a reason.
use std::{cell::RefCell, path::Path, rc::Rc};

use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_file_history::{FileHistoryEvent, FileHistoryView};

use crate::macos::{build_fixture, git, repo_fingerprint, unmount};
use crate::recovery_layout::draw;

pub(crate) fn open_history(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    path: &str,
) -> Entity<FileHistoryView> {
    app.update(cx, |app, cx| app.open_file_history(path.into(), None, cx));
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, window, dimensions);
    }
    cx.read(|cx| app.read(cx).ui().file_history.clone().unwrap())
}

fn click(
    cx: &mut VisualTestAppContext,
    pane: &Entity<FileHistoryView>,
    window: AnyWindowHandle,
    id: &str,
) {
    let bounds = cx.read(|cx| pane.read(cx).header_fit.control_bounds(id).unwrap());
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

fn observe(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    pane: &Entity<FileHistoryView>,
) -> (Rc<RefCell<Vec<String>>>, gpui::Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let sub = app.update(cx, |_, cx| {
        cx.subscribe(pane, move |_, _, event: &FileHistoryEvent, _| {
            output.borrow_mut().push(format!("{event:?}"));
        })
    });
    (events, sub)
}

fn assert_refused(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, reason: &str) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.ui().file_history.is_some(),
            "unavailable file keeps History open"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        assert_eq!(toast.message.as_ref(), reason);
    });
}

fn check_retained(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<kagi_ui_editor::EditorWorkspaceView>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, window, dimensions);
    }
    let live = cx.read(|cx| editor.read(cx).editor.clone()).unwrap();
    cx.update_window(window, |_, window, cx| {
        use gpui::Focusable;
        window.focus(&live.read(cx).focus_handle(cx), cx);
    })
    .unwrap();
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).any_dirty()));
    // A retained Editor behind PRs must be revealed, not merely have its path changed.
    app.update(cx, |app, cx| app.show_pr_mode(cx));

    let pane = open_history(cx, app, window, dimensions, "renamed.txt");
    click(cx, &pane, window, "fh-open-file");
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        kagi::ui::workspace_mode::WorkspaceMode::Editor,
        "Open File must reveal a retained Editor, not return to PRs"
    );
    assert_eq!(
        cx.read(|cx| editor.read(cx).open_path.clone()),
        Some("renamed.txt".into())
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .editor_workspace
            .as_ref()
            .unwrap()
            .entity_id()),
        editor.entity_id(),
        "reuse existing Editor"
    );
    assert!(
        cx.read(|cx| editor.read(cx).any_dirty()),
        "opening from History preserves dirty tabs"
    );
    assert_eq!(cx.read(|cx| live.read(cx).value().to_string()), "x");
    drop(pane);
}

fn check_unavailable(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<kagi_ui_editor::EditorWorkspaceView>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    // Followed history's pre-rename entry must not silently open the latest path.
    let pane = open_history(cx, app, window, dimensions, "renamed.txt");
    pane.update(cx, |pane, cx| {
        let index = pane
            .data
            .history
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .position(|entry| entry.change.path_after == Path::new("original.txt"))
            .unwrap();
        pane.select(index, cx);
    });
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, window, dimensions);
    }
    click(cx, &pane, window, "fh-open-file");
    let reason = match i18n::lang() {
        Lang::En => "File was renamed; open renamed.txt instead.",
        Lang::Ja => "ファイル名が変更されています。renamed.txt を開いてください。",
    };
    assert_refused(cx, app, reason);
    assert_eq!(
        cx.read(|cx| editor.read(cx).open_path.clone()),
        Some("renamed.txt".into())
    );
    drop(pane);

    for (path, reason) in [
        ("deleted.txt", Msg::EditorWorkspaceDeleted),
        ("binary.dat", Msg::EditorWorkspaceBinary),
    ] {
        let pane = open_history(cx, app, window, dimensions, path);
        let (events, sub) = observe(cx, app, &pane);
        click(cx, &pane, window, "fh-open-file");
        assert_refused(cx, app, reason.t());
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(
            cx.read(|cx| editor.read(cx).open_path.clone()),
            Some("renamed.txt".into())
        );
        drop((pane, sub));
    }
    app.update(cx, |app, _| app.close_editor_workspace());
    let pane = open_history(cx, app, window, dimensions, "deleted.txt");
    click(cx, &pane, window, "fh-open-file");
    assert_refused(cx, app, Msg::EditorWorkspaceDeleted.t());
    assert!(
        cx.read(|cx| app.read(cx).ui().editor_workspace.is_none()),
        "failed open creates no Editor"
    );
    drop(pane);
}

fn check_image(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<kagi_ui_editor::EditorWorkspaceView>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    let pane = open_history(cx, app, window, dimensions, "preview.png");
    let (events, sub) = observe(cx, app, &pane);
    click(cx, &pane, window, "fh-open-file");
    assert!(
        cx.read(|cx| app.read(cx).ui().file_history.is_none()),
        "Open File must admit a PNG that the Editor can preview"
    );
    assert_eq!(events.borrow().len(), 1);
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, window, dimensions);
    }
    cx.read(|cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.open_path.as_deref(), Some(Path::new("preview.png")));
        assert!(
            editor.content_image.is_some(),
            "Editor loaded the real PNG preview"
        );
        assert!(editor.any_dirty(), "image navigation keeps dirty text tabs");
    });
    drop((pane, sub));
    let pane = open_history(cx, app, window, dimensions, "renamed.txt");
    click(cx, &pane, window, "fh-open-file");
    drop(pane);
}

fn check_case(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    dimensions: (f32, f32),
    preset: &str,
    lang: Lang,
) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |app, cx| app.set_theme(preset, cx));
    i18n::set_lang(lang);
    let pane = open_history(cx, &app, window, dimensions, "README.md");
    let (events, sub) = observe(cx, &app, &pane);
    click(cx, &pane, window, "fh-back");
    assert_eq!(*events.borrow(), ["CloseRequested"]);
    assert!(cx.read(|cx| app.read(cx).ui().editor_workspace.is_none()));
    drop((pane, sub));

    let pane = open_history(cx, &app, window, dimensions, "README.md");
    let (events, sub) = observe(cx, &app, &pane);
    click(cx, &pane, window, "fh-open-file");
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("Open File must create the Editor workspace, not only close History");
    assert!(cx.read(|cx| app.read(cx).ui().file_history.is_none()));
    assert_eq!(
        cx.read(|cx| editor.read(cx).open_path.clone()),
        Some("README.md".into())
    );
    assert_eq!(
        events.borrow().len(),
        1,
        "one outward open request per click"
    );
    assert!(events.borrow()[0].starts_with("OpenFileRequested"));
    drop((pane, sub));
    check_retained(cx, &app, &editor, window, dimensions);
    check_image(cx, &app, &editor, window, dimensions);
    check_unavailable(cx, &app, &editor, window, dimensions);

    drop((editor, captured));
    unmount(cx, app, window);
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("original.txt"), "rename content\n").unwrap();
    std::fs::write(repo.join("deleted.txt"), "deleted content\n").unwrap();
    std::fs::write(repo.join("binary.dat"), b"binary\0data").unwrap();
    std::fs::write(
        repo.join("preview.png"),
        crate::diff_highlight::png_bytes([255, 0, 0, 255]),
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "history open fixture"]);
    git(repo, &["mv", "original.txt", "renamed.txt"]);
    git(repo, &["rm", "deleted.txt"]);
    git(repo, &["commit", "-qm", "rename and delete"]);
    let before = repo_fingerprint(repo);
    for preset in ["apple-light", "apple-dark"] {
        for lang in [Lang::En, Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for dimensions in [(940., 660.), (1392., 883.)] {
                    check_case(cx, repo, dimensions, preset, lang);
                    assert_eq!(
                        before,
                        repo_fingerprint(repo),
                        "Open and Back perform no Git write"
                    );
                }
            }
        }
    }
}
