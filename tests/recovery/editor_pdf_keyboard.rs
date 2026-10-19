//! #1680: real PDF body click/Tab and key dispatch, never scroll action callbacks.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::native_ax;
use crate::pr_viewed::{paint, rev};
use gpui::{
    AnyWindowHandle, Entity, Focusable as _, Modifiers, Pixels, Point, VisualTestAppContext,
};
use gpui_component::WindowExt as _;
use kagi::ui::{editor_workspace::EditorWorkspaceView, KagiApp};
use kagi_ui_core::{
    file_tree::TreeRow,
    i18n::{self, Lang, Msg},
};
use kagi_ui_editor::{PdfProbe, TreeSource};
use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

// Signature classification lets the same retained buffer become ordinary text.
const PDF_PATH: &str = "a-reading";
const NEXT: &str = "b-next.txt";
const PDF: &[u8] = include_bytes!("../fixtures/pdf/document.pdf");
const NOTE: &[u8] = b"unchanged next file\n";
const BODY: &str = "ews-pdf-page-scroll";

fn wait_for(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx))) {
            paint(cx, window);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PDF keyboard fixture did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn wait_pdf(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) -> PdfProbe {
    wait_for(cx, editor, window, |view| {
        !view.file_loading && view.pdf_probe().is_some_and(|pdf| pdf.settled)
    });
    cx.read(|cx| editor.read(cx).pdf_probe().unwrap())
}

fn click_file(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    path: &str,
) {
    paint(cx, window);
    let bounds = cx.read(|cx| {
        let view = editor.read(cx);
        let file = view
            .files
            .iter()
            .position(|file| file.path == Path::new(path))
            .unwrap();
        let row = view
            .tree
            .iter()
            .position(|row| matches!(row, TreeRow::File { file_index, .. } if *file_index == file))
            .unwrap();
        view.tree_row_probe_for_e2e(row)
            .unwrap()
            .row
            .unwrap()
            .bounds
    });
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    paint(cx, window);
}

fn focused_id(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Option<String> {
    let tree = native_ax::tree(cx, window);
    tree["gpui_focus"]
        .as_str()
        .and_then(|id| tree["nodes"][id]["element_id"].as_str())
        .map(str::to_owned)
}

fn tab_to(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &'static str) {
    let identity = format!("{:?}", gpui::ElementId::from(id));
    for _ in 0..160 {
        keys(cx, window, "tab");
        paint(cx, window);
        if focused_id(cx, window).as_deref() == Some(identity.as_str()) {
            return;
        }
    }
    panic!(
        "ordinary Tab must reach {id}: {}",
        native_ax::tree(cx, window)
    );
}

fn click_body(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let bounds = cx.read(|cx| editor.read(cx).pdf_probe().unwrap().scroll.bounds());
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    paint(cx, window);
}

fn assert_open(cx: &mut VisualTestAppContext, editor: &Entity<EditorWorkspaceView>, path: &str) {
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(
            view.open_path.as_deref(),
            Some(Path::new(path)),
            "keyboard routing changed the open file"
        );
        assert_eq!(
            view.selected.map(|i| view.files[i].path.as_path()),
            Some(Path::new(path)),
            "keyboard routing changed the Tree selection"
        );
    });
}

// Every scrolling key must preserve the real document/page/raster request as
// well as the highlighted and open file, including repeated boundary input.
fn press_body(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    initial: &PdfProbe,
    key: &str,
) -> Point<Pixels> {
    keys(cx, window, key);
    paint(cx, window);
    assert_open(cx, editor, PDF_PATH);
    let current = cx.read(|cx| editor.read(cx).pdf_probe().expect("body key discarded PDF"));
    assert_eq!(
        (
            current.page,
            current.page_count,
            current.zoom,
            current.request
        ),
        (
            initial.page,
            initial.page_count,
            initial.zoom,
            initial.request
        ),
        "body key {key} changed document/page/zoom/request"
    );
    assert!(
        Arc::ptr_eq(
            current.image.as_ref().unwrap(),
            initial.image.as_ref().unwrap()
        ),
        "scrolling must keep the accepted page bitmap"
    );
    let offset = current.scroll.offset();
    let maximum = current.scroll.max_offset();
    assert!(
        offset.x >= -maximum.x
            && offset.x <= gpui::px(0.)
            && offset.y >= -maximum.y
            && offset.y <= gpui::px(0.),
        "{key} escaped current viewport bounds: {offset:?}, max {maximum:?}"
    );
    offset
}

fn assert_close(actual: Pixels, expected: Pixels, why: &str) {
    assert!(
        (actual - expected).abs() < gpui::px(1.),
        "{why}: {actual:?}, expected {expected:?}"
    );
}

fn scroll_keys(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    initial: &PdfProbe,
) {
    let start = initial.scroll.offset();
    // Expected failing-before point: body click focuses the shared app root;
    // Down opens b-next.txt instead of moving this viewport.
    let down = press_body(cx, editor, window, initial, "down");
    assert!(
        down.y < start.y,
        "PDF body click + Down must visibly scroll the current page"
    );
    assert_eq!(down.x, start.x, "Down is vertical only");
    let up = press_body(cx, editor, window, initial, "up");
    assert!(up.y > down.y, "Up must reverse vertical movement");
    let right = press_body(cx, editor, window, initial, "right");
    assert!(
        right.x < up.x,
        "Right must reveal horizontally clipped PDF content"
    );
    assert_eq!(right.y, up.y, "Right is horizontal only");
    let left = press_body(cx, editor, window, initial, "left");
    assert!(left.x > right.x, "Left must reverse horizontal movement");

    press_body(cx, editor, window, initial, "home");
    let page = press_body(cx, editor, window, initial, "pagedown");
    let height = initial.scroll.bounds().size.height;
    assert!(
        -page.y >= height / 2. && -page.y <= height + gpui::px(1.),
        "PageDown must move a useful viewport, not one line: {page:?}, height {height:?}"
    );
    let previous = press_body(cx, editor, window, initial, "pageup");
    assert!(previous.y > page.y, "PageUp reverses page movement");
    let space = press_body(cx, editor, window, initial, "space");
    assert!(
        previous.y - space.y >= height / 2.,
        "Space moves a useful viewport"
    );
    let back = press_body(cx, editor, window, initial, "shift-space");
    assert!(back.y > space.y, "Shift+Space reverses page movement");

    for key in ["end", "cmd-down"] {
        press_body(cx, editor, window, initial, "home");
        let bottom = press_body(cx, editor, window, initial, key);
        assert_close(
            bottom.y,
            -initial.scroll.max_offset().y,
            "End/Cmd+Down reaches current page bottom",
        );
        for boundary in ["down", "pagedown", "space", "cmd-down", "end"] {
            assert_eq!(
                press_body(cx, editor, window, initial, boundary),
                bottom,
                "bottom boundary key {boundary} must be consumed, not select another file/page"
            );
        }
    }
    for key in ["home", "cmd-up"] {
        press_body(cx, editor, window, initial, "end");
        let top = press_body(cx, editor, window, initial, key);
        assert_close(top.y, gpui::px(0.), "Home/Cmd+Up reaches current page top");
        for boundary in ["up", "pageup", "shift-space", "cmd-up", "home"] {
            assert_eq!(
                press_body(cx, editor, window, initial, boundary),
                top,
                "top boundary key {boundary} must be consumed"
            );
        }
    }
    for (key, target) in [
        ("right", -initial.scroll.max_offset().x),
        ("left", gpui::px(0.)),
    ] {
        let mut reached = false;
        for _ in 0..128 {
            let offset = press_body(cx, editor, window, initial, key);
            if (offset.x - target).abs() < gpui::px(1.) {
                assert_eq!(
                    press_body(cx, editor, window, initial, key),
                    offset,
                    "horizontal boundary must be consumed"
                );
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "repeated {key} must reach and clamp to the horizontal edge"
        );
    }
}

fn mount_compact(cx: &mut VisualTestAppContext, repo: &Path) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = kagi::ui::e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        gpui::size(gpui::px(940.), gpui::px(660.)),
        move |window, cx| kagi::ui::e2e::mount_root(state, window, cx, &output),
    );
    crate::gui_evidence::fixture(repo);
    let app = captured.borrow().clone().unwrap();
    (app, *window)
}

fn modal_and_reclassification(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    repo: &Path,
    head: &str,
) {
    click_body(cx, editor, window);
    let retained = cx.read(|cx| editor.read(cx).pdf_probe().unwrap());
    let offset = retained.scroll.offset();
    app.update(cx, |app, cx| {
        app.open_create_branch_modal(kagi_git::CommitId(head.to_owned()), cx)
    });
    paint(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| window.has_focused_input(cx))
            .unwrap(),
        "modal Input must own focus above the PDF"
    );
    for key in ["down", "pagedown", "end", "space", "shift-space"] {
        keys(cx, window, key);
        paint(cx, window);
        assert_open(cx, editor, PDF_PATH);
        assert_eq!(
            retained.scroll.offset(),
            offset,
            "modal-owned {key} must not scroll the background PDF"
        );
        assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_some()));
    }
    // A watcher reclassification while the modal owns focus must not reclaim
    // root/body focus. Prove this by editing the actual modal field afterward.
    std::fs::write(repo.join(PDF_PATH), b"now ordinary text\n").unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    wait_for(cx, editor, window, |view| {
        !view.file_loading && view.pdf_probe().is_none() && view.editor.is_some()
    });
    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .unwrap()
                .input_state
                .clone()
        })
        .unwrap();
    assert!(
        cx.update_window(window, |_, window, cx| input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
            .unwrap(),
        "PDF disappearing must preserve the modal's focus"
    );
    keys(cx, window, "cmd-a");
    cx.simulate_keystrokes(window, "retained");
    paint(cx, window);
    cx.read(|cx| {
        assert_eq!(
            input.read(cx).value().as_ref(),
            "retained",
            "keys must reach the foreground modal Input after PDF reclassification"
        )
    });
    keys(cx, window, "escape");
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    drop(input);

    // Repeat reclassification with the body itself focused, not just a modal
    // above it. The next Down must reach ordinary Tree routing, not a hidden
    // PDF handle whose dispatch ancestry has disappeared.
    std::fs::write(repo.join(PDF_PATH), PDF).unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    wait_pdf(cx, editor, window);
    click_body(cx, editor, window);
    std::fs::write(repo.join(PDF_PATH), b"now ordinary text again\n").unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    wait_for(cx, editor, window, |view| {
        !view.file_loading && view.pdf_probe().is_none() && view.editor.is_some()
    });
    keys(cx, window, "down");
    paint(cx, window);
    assert_open(cx, editor, NEXT);
    std::fs::write(repo.join(PDF_PATH), PDF).unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
}

fn workspace_takeovers(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    click_file(cx, editor, window, PDF_PATH);
    let retained = wait_pdf(cx, editor, window);
    click_body(cx, editor, window);
    keys(cx, window, "cmd-t");
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).home_in_front()));
    let offset = retained.scroll.offset();
    keys(cx, window, "pagedown");
    paint(cx, window);
    assert_eq!(
        retained.scroll.offset(),
        offset,
        "Home must not scroll a retained hidden PDF"
    );
    keys(cx, window, "cmd-w");
    paint(cx, window);
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    wait_pdf(cx, editor, window);
    click_body(cx, editor, window);
    keys(cx, window, "cmd-shift-g");
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).ui().editor_workspace.is_none()));
    keys(cx, window, "home");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "Graph must accept keyboard navigation after the focused PDF closes"
    );
    keys(cx, window, "down");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(1),
        "Down must move Graph selection, not an orphaned PDF viewport"
    );
}

pub fn scenario_editor_pdf_keyboard(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    for (lang, zoom) in [(Lang::En, 1.), (Lang::Ja, 1.67)] {
        i18n::set_lang(lang);
        kagi_ui_core::theme::set_zoom(zoom);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::write(repo.join(PDF_PATH), PDF).unwrap();
        std::fs::write(repo.join(NEXT), NOTE).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "PDF keyboard fixture"]);
        let head = rev(&repo, "HEAD");
        let index = git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]);
        let index_bytes = std::fs::read(repo.join(".git/index")).unwrap();
        let readme = std::fs::read(repo.join("README.md")).unwrap();
        let (app, window) = if zoom > 1. {
            mount_compact(cx, &repo)
        } else {
            mount(cx, &repo)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if zoom > 1. {
                keys(cx, window, "cmd-j");
            }
            app.update(cx, |app, cx| app.open_editor_workspace(cx));
            let editor = cx
                .read(|cx| app.read(cx).ui().editor_workspace.clone())
                .unwrap();
            editor.update(cx, |view, cx| view.set_source(TreeSource::All, cx));
            wait_for(cx, &editor, window, |view| {
                !view.loading
                    && view
                        .files
                        .iter()
                        .any(|file| file.path == Path::new(PDF_PATH))
            });
            click_file(cx, &editor, window, PDF_PATH);
            wait_pdf(cx, &editor, window);
            keys(cx, window, "down");
            paint(cx, window);
            assert_open(cx, &editor, NEXT);
            click_file(cx, &editor, window, PDF_PATH);
            wait_pdf(cx, &editor, window);
            native_ax::activate(cx);
            // Real toolbar Space prepares two-axis overflow, without setting
            // offsets, calling change_pdf, or bypassing the production keymap.
            tab_to(cx, window, "ews-pdf-zoom-in");
            for _ in 0..4 {
                keys(cx, window, "space");
                wait_pdf(cx, &editor, window);
            }
            let first = wait_pdf(cx, &editor, window);
            assert_eq!((first.page, first.page_count), (0, 3));
            assert!(first.image.is_some() && first.error.is_none());
            assert!(
                first.scroll.max_offset().x > gpui::px(100.)
                    && first.scroll.max_offset().y > first.scroll.bounds().size.height,
                "fixture must overflow both axes by more than one vertical viewport"
            );
            click_body(cx, &editor, window);
            scroll_keys(cx, &editor, window, &first);

            // Leave body focus for an ordinary toolbar stop. Space must activate
            // Next, rather than being swallowed by the body scroll bindings.
            tab_to(cx, window, "ews-pdf-next");
            keys(cx, window, "space");
            let second = wait_pdf(cx, &editor, window);
            assert_open(cx, &editor, PDF_PATH);
            assert_eq!(
                second.page, 1,
                "toolbar Space changes page, not PDF body scroll"
            );
            assert_close(
                second.scroll.offset().y,
                gpui::px(0.),
                "toolbar page navigation starts at top",
            );
            tab_to(cx, window, BODY);
            let body = native_ax::focused(cx, window);
            assert_eq!(body["aria"]["role"], "Group");
            assert_eq!(body["aria"]["label"], Msg::EditorPdfReadOnly.t());
            let before = second.scroll.offset();
            let after = press_body(cx, &editor, window, &second, "space");
            assert!(
                after.y < before.y,
                "ordinary Tab entry must give Space to the PDF viewport"
            );
            assert_eq!(
                std::fs::read(repo.join(".git/index")).unwrap(),
                index_bytes,
                "keyboard viewing must preserve the fixture index bytes"
            );
            // The following lifecycle checks deliberately rewrite the fixture.
            // Home return may then repair stat-cache metadata (ADR-0193);
            // staged paths/OIDs/modes and document bytes must still survive.
            modal_and_reclassification(cx, &app, &editor, window, &repo, &head);
            workspace_takeovers(cx, &app, &editor, window);
            assert_eq!(
                rev(&repo, "HEAD"),
                head,
                "reading/navigation must not mutate HEAD"
            );
            assert_eq!(
                git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]),
                index,
                "reading/navigation preserves staged paths, OIDs and modes"
            );
            assert_eq!(std::fs::read(repo.join(PDF_PATH)).unwrap(), PDF);
            assert_eq!(std::fs::read(repo.join(NEXT)).unwrap(), NOTE);
            assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), readme);
            drop(editor);
        }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
