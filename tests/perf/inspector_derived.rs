//! Issue #512: the Inspector's file tree and message HTML are derived when
//! their inputs change, never per draw. Included by `gui_e2e_runner` — it needs
//! the production `render_inspector` path, real clicks and real draws.
//!
//! The oracle is the temporary derivation counter behind the `gui-e2e`
//! feature (`KagiApp::inspector_derivations_for_e2e`): draws with unchanged inputs keep
//! it still; a selection, a reload and a compare move it. The same run also
//! proves the display those derivations feed: the ">100 files" tail, the
//! generated fold, the row click target, and the file menu's Copy Path.
//!
//!   KAGI_GUI_E2E=1 KAGI_GUI_E2E_ONLY=inspector_derived cargo test -p kagi \
//!     --features gui-e2e --test gui_e2e_runner -- --nocapture

use std::path::PathBuf;

use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{diff_view::MainDiffSource, e2e, KagiApp};

use crate::macos::{git, mount, unmount};

/// `(file-list derivations, message HTML conversions)` so far.
fn derivations() -> (usize, usize) {
    KagiApp::inspector_derivations_for_e2e()
}

/// Source files in the wide commit; with `Cargo.lock` that is 103 changed
/// files, three past the Inspector's 100-row cap.
const WIDE_SOURCES: usize = 102;

/// `initial commit` ← `wide commit` (HEAD): `Cargo.lock` + 102 sources.
fn wide_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("README.md"), "# fixture\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "initial commit"]);
    std::fs::create_dir(p.join("src")).unwrap();
    for i in 0..WIDE_SOURCES {
        std::fs::write(p.join(format!("src/m{i:03}.rs")), format!("// {i}\n")).unwrap();
    }
    std::fs::write(p.join("Cargo.lock"), "# @generated\nversion = 3\n").unwrap();
    git(p, &["add", "."]);
    git(
        p,
        &[
            "commit",
            "-q",
            "-m",
            "wide commit\n\nA hard-wrapped body line that git's 72-column\nconvention split.",
        ],
    );
    dir
}

/// Draw one real frame and return the bounds of `id` in it.
fn draw(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(win.window_id(), id);
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw window");
    e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("{id} was not drawn"))
}

/// Draw five frames with unchanged inputs; no derivation may run.
fn assert_stable(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, what: &str) {
    let before = derivations();
    for _ in 0..5 {
        draw(cx, win, id);
    }
    assert_eq!(
        derivations(),
        before,
        "{what}: a draw with unchanged inputs re-derived the file tree or message HTML"
    );
}

fn selected_path(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, fi: usize) -> PathBuf {
    cx.read(|cx| {
        let ui = app.read(cx).ui();
        let row = ui.selected.expect("a selected commit");
        ui.diff_caches.changed_files()[&row]
            .as_ref()
            .expect("changed files loaded")[fi]
            .path
            .clone()
    })
}

fn wheel(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    at: gpui::Point<gpui::Pixels>,
    dy: f32,
) {
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}

pub fn scenario_inspector_derived(cx: &mut VisualTestAppContext) {
    let fixture = wide_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();

    // >100 files: the tail row renders, and the generated fold holds Cargo.lock.
    draw(cx, win, "inspector-files-more");
    draw(cx, win, "generated-fold-header");
    assert_stable(cx, win, "inspector-files-more", "tree view");

    // Display options choose among derived rows; they re-derive nothing.
    let before = derivations();
    app.update(cx, |app, cx| {
        app.inspector_tree_view = false;
        cx.notify();
    });
    // The fold header sits below 100 rows: scroll the real file list to it.
    let list = draw(cx, win, "inspector-file-5");
    wheel(cx, win, list.center(), -100_000.);
    let header = draw(cx, win, "generated-fold-header");
    cx.simulate_click(win, header.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).inspector_generated_expanded));
    draw(cx, win, "inspector-file-0");
    assert_eq!(
        derivations(),
        before,
        "Path⇄Tree and the Generated fold must not re-derive the model"
    );
    assert!(selected_path(cx, &app, 0).ends_with("Cargo.lock"));
    wheel(cx, win, header.center(), 100_000.);

    // Click target: the row drawn for file 5 opens file 5's diff.
    let row = draw(cx, win, "inspector-file-5");
    cx.simulate_click(win, row.center(), Modifiers::none());
    cx.run_until_parked();
    let opened = cx.read(|cx| {
        let pane = app
            .read(cx)
            .ui()
            .main_diff
            .clone()
            .expect("row click opens a diff");
        match pane.read(cx).view.source {
            MainDiffSource::Commit { file_index, .. } => file_index,
            _ => panic!("a commit row click opened a non-commit diff"),
        }
    });
    assert_eq!(opened, 5);

    // Context menu + Copy Path resolve the right-clicked row's own path.
    let row = draw(cx, win, "inspector-file-7");
    cx.simulate_mouse_down(
        win,
        row.center(),
        gpui::MouseButton::Right,
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).inspector_file_menu.map(|(fi, _)| fi)),
        Some(7)
    );
    let copy = draw(cx, win, "insp-menu-copy");
    cx.simulate_click(win, copy.center(), Modifiers::none());
    cx.run_until_parked();
    let expected = selected_path(cx, &app, 7);
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(expected.to_string_lossy().into_owned())
    );

    // Selection: the new commit's list and message are each derived once
    // (the list when its load lands), then draws settle again.
    let before = derivations();
    app.update(cx, |app, cx| {
        app.close_main_diff();
        app.select(1);
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, win, "inspector-file-0");
    assert_eq!(
        derivations(),
        (before.0 + 1, before.1 + 1),
        "a selection derives the new commit's list and message once each"
    );
    assert!(selected_path(cx, &app, 0).ends_with("README.md"));
    assert_stable(cx, win, "inspector-file-0", "after selection");

    // Reload: an external commit renumbers rows and clears the caches. The
    // same commit's files re-derive; its message (same SHA) does not.
    let before = derivations();
    std::fs::write(repo.join("README.md"), "# fixture\nmore\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "external commit"]);
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    draw(cx, win, "inspector-file-0");
    let after = derivations();
    assert!(after.0 > before.0, "a reload must re-derive the file list");
    assert_eq!(
        after.1, before.1,
        "the same commit's message is not re-converted"
    );
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(2));
    assert!(selected_path(cx, &app, 0).ends_with("README.md"));
    assert_stable(cx, win, "inspector-file-0", "after reload");

    // Compare (of the selected commit, so the message stays): its own file
    // source derives once, then settles.
    std::fs::write(repo.join("README.md"), "# fixture\nwip\n").unwrap();
    let before = derivations();
    app.update(cx, |app, cx| {
        let selected = app.view().rows[2].id.clone();
        app.open_compare_with_working_tree(selected, Some(cx));
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().compare_view.is_some()));
    draw(cx, win, "inspector-file-0");
    assert_eq!(
        derivations(),
        (before.0 + 1, before.1),
        "opening a compare derives its file list once"
    );
    assert_stable(cx, win, "inspector-file-0", "compare");

    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS inspector_derived");
}
