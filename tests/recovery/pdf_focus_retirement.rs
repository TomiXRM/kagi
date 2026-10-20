//! Historical PDF bodies retire through the shared post-paint focus guard.
use crate::diff_highlight::{commit_image_index, settle};
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::native_ax;
use crate::pr_viewed::{paint, rev};
use gpui::{AnyWindowHandle, Entity, FocusHandle, Focusable as _, Modifiers, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_git::CommitId;
use std::time::{Duration, Instant};

const PDF: &str = "reading.pdf";
const OTHER: &str = "replacement.pdf";
const NOTE: &str = "note.txt";
const DOCUMENT: &[u8] = include_bytes!("../fixtures/pdf/document.pdf");
const ROTATED: &[u8] = include_bytes!("../fixtures/pdf/rotated.pdf");

fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    for (path, bytes) in [
        (PDF, DOCUMENT),
        (OTHER, ROTATED),
        (NOTE, b"old text\n".as_slice()),
    ] {
        std::fs::write(repo.join(path), bytes).unwrap();
    }
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "Previous documents"]);
    for (path, bytes) in [
        (PDF, ROTATED),
        (OTHER, DOCUMENT),
        (NOTE, b"new text\n".as_slice()),
    ] {
        std::fs::write(repo.join(path), bytes).unwrap();
    }
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "Current documents"]);
    fixture
}

fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle, path: &str) {
    let index = commit_image_index(cx, app, path);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        paint(cx, window);
        if cx.read(|cx| {
            app.read(cx)
                .ui()
                .main_diff
                .as_ref()
                .is_some_and(|pane| pane.read(cx).view.title.as_ref() == path)
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "historical pane did not open: {path}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn click_body(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    old: bool,
) -> FocusHandle {
    let viewer = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .unwrap()
            .read(cx)
            .pdf_viewer_for_e2e(old, cx)
            .expect("real historical PDF viewer")
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let bounds = loop {
        cx.advance_clock(Duration::from_millis(16));
        paint(cx, window);
        let pdf = cx.read(|cx| viewer.read(cx).pdf_probe());
        if pdf.settled && pdf.image.is_some() {
            assert!(pdf.error.is_none());
            break pdf.scroll.bounds();
        }
        assert!(
            Instant::now() < deadline,
            "historical PDF did not rasterize"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.));
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    paint(cx, window);
    let focus = cx.read(|cx| viewer.read(cx).focus_handle(cx));
    cx.update_window(window, |_, window, _| {
        assert!(
            focus.is_focused(window),
            "actual PDF body click must own focus"
        );
    })
    .unwrap();
    focus
}

fn assert_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        assert!(
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window),
            "retired PDF focus must return to the drawn workspace without a repair click"
        );
    })
    .unwrap();
}

fn graph_keys(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    keys(cx, window, "home");
    paint(cx, window);
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
    keys(cx, window, "down");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(1),
        "the first Graph arrow after retirement must dispatch without a repair click"
    );
    keys(cx, window, "up");
    settle(cx, window);
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
}

pub fn scenario_pdf_focus_retirement(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);

    // Both independent bodies must survive Escape -> Graph keyboard dispatch.
    // No pane/viewer entity is retained by the test when it closes.
    for old in [true, false] {
        open(cx, &app, window, PDF);
        let retired = click_body(cx, &app, window, old);
        keys(cx, window, "escape");
        paint(cx, window);
        assert!(cx.read(|cx| app.read(cx).ui().main_diff.is_none()));
        assert_root(cx, &app, window);
        cx.update_window(window, |_, window, _| assert!(!retired.is_focused(window)))
            .unwrap();
        graph_keys(cx, &app, window);
    }

    // Replacement is not an Escape close: both a different PDF pair and a text
    // presentation remove the focused child of the retained MainDiffPane.
    for replacement in [OTHER, NOTE] {
        open(cx, &app, window, PDF);
        let retired = click_body(cx, &app, window, false);
        open(cx, &app, window, replacement);
        assert_root(cx, &app, window);
        cx.update_window(window, |_, window, _| assert!(!retired.is_focused(window)))
            .unwrap();
        app.update(cx, |app, cx| {
            app.close_main_diff();
            cx.notify();
        });
        paint(cx, window);
        graph_keys(cx, &app, window);
    }

    // Home backgrounds the repository owner but retains its pane and viewers.
    open(cx, &app, window, PDF);
    let retired = click_body(cx, &app, window, true);
    keys(cx, window, "cmd-t");
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).home_in_front()));
    assert_root(cx, &app, window);
    cx.update_window(window, |_, window, _| assert!(!retired.is_focused(window)))
        .unwrap();
    keys(cx, window, "cmd-w");
    paint(cx, window);
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    assert!(cx.read(|cx| app.read(cx).ui().main_diff.is_some()));
    // A windowless close also goes through membership, not an Escape handler.
    click_body(cx, &app, window, false);
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    paint(cx, window);
    assert_root(cx, &app, window);
    graph_keys(cx, &app, window);
    assert_eq!(
        before,
        repo_fingerprint(&repo),
        "PDF focus transitions are read-only"
    );
    unmount(cx, app, window);
}

pub fn scenario_pdf_focus_retirement_preserves_input(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    open(cx, &app, window, PDF);
    click_body(cx, &app, window, false);
    keys(cx, window, "secondary-f");
    paint(cx, window);
    let input = cx.read(|cx| app.read(cx).sidebar.filter.clone()).unwrap();
    cx.update_window(window, |_, window, cx| {
        assert!(input.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        assert!(
            input.read(cx).focus_handle(cx).is_focused(window),
            "a retired PDF must not steal unrelated sidebar Input focus"
        );
    })
    .unwrap();
    keys(cx, window, "cmd-a");
    cx.simulate_keystrokes(window, "retained");
    paint(cx, window);
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), "retained");
    keys(cx, window, "cmd-a");
    keys(cx, window, "backspace");
    paint(cx, window);

    open(cx, &app, window, PDF);
    click_body(cx, &app, window, true);
    app.update(cx, |app, cx| {
        app.open_create_branch_modal(CommitId(rev(&repo, "HEAD")), cx)
    });
    paint(cx, window);
    let modal_input = cx.read(|cx| {
        app.read(cx)
            .create_branch_modal()
            .unwrap()
            .input_state
            .clone()
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        assert!(modal_input.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        assert!(
            modal_input.read(cx).focus_handle(cx).is_focused(window),
            "modal Input keeps focus when its background PDF disappears"
        );
    })
    .unwrap();
    keys(cx, window, "cmd-a");
    cx.simulate_keystrokes(window, "retainedmodal");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| modal_input.read(cx).value().to_string()),
        "retainedmodal"
    );
    keys(cx, window, "escape");
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    assert_eq!(
        before,
        repo_fingerprint(&repo),
        "focus recovery must not execute a Git operation"
    );
    unmount(cx, app, window);
}
