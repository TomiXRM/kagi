//! #809: diff and File History header buttons stay inside the window.
//!
//! At 1000×720 and zoom 1.25 the Main Diff header's Back / external editor /
//! History / view-mode buttons and its +N −M, and the File History header's
//! Back / Refresh / Copy Path / Open File / Follow Renames, were laid out past
//! the centre pane and clipped. The oracle is each control's own laid-out
//! bounds (recorded by the production `HeaderFit`), inside its header row and
//! the row inside the window, in EN and JA. A wide window keeps the labels.
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, VisualTestAppContext};
use kagi::ui::file_history::FhDiffPane;
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::header_fit::HeaderFit;
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};

const LONG_PATH: &str =
    "src/some/deeply/nested/directory/with_a_rather_long_file_name_for_truncation.rs";
const DIFF_CONTROLS: [&str; 5] = [
    "main-diff-back",
    "main-diff-ext-editor",
    "main-diff-history",
    "diff-mode-toggle",
    "main-diff-stats",
];
const FH_CONTROLS: [&str; 5] = [
    "fh-back",
    "fh-refresh",
    "fh-copy-path",
    "fh-open-file",
    "fh-follow",
];
const FH_DIFF_CONTROLS: [&str; 2] = ["diff-mode-toggle", "main-diff-stats"];

/// Draw until the header settles: a changed fit asks for one more frame.
fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, win, dimensions);
    }
}

fn assert_fits(fit: &HeaderFit, controls: &[&str], dimensions: (f32, f32), label: &str) {
    let window = Bounds::new(
        point(px(0.), px(0.)),
        size(px(dimensions.0), px(dimensions.1)),
    );
    let row = fit
        .row_bounds()
        .unwrap_or_else(|| panic!("{label}: header row not laid out"));
    contained(window, row, &format!("{label}/row"));
    for name in controls {
        let control = fit
            .control_bounds(name)
            .unwrap_or_else(|| panic!("{label}/{name}: control not laid out"));
        assert!(
            control.size.width > px(0.),
            "{label}/{name}: empty {control:?}"
        );
        contained(row, control, &format!("{label}/{name}"));
    }
}

fn open_window(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
) -> (gpui::Entity<KagiApp>, AnyWindowHandle) {
    let app_state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(app_state, window, cx, &output),
    );
    let app = captured.borrow().clone().expect("captured KagiApp");
    cx.run_until_parked();
    (app, win.into())
}

fn check(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
    label: &str,
) -> bool {
    let (app, win) = open_window(cx, repo, dimensions);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    settle(cx, win, dimensions);
    let diff = cx
        .read(|cx| app.read(cx).ui().main_diff.clone())
        .expect("main diff open");
    let diff_fit = cx.read(|cx| diff.read(cx).fit.clone());
    assert_fits(
        &diff_fit,
        &DIFF_CONTROLS,
        dimensions,
        &format!("{label}/diff"),
    );
    let compact = diff_fit.compact();

    app.update(cx, |app, cx| {
        app.open_file_history(PathBuf::from(LONG_PATH), None, cx)
    });
    settle(cx, win, dimensions);
    let pane = cx
        .read(|cx| app.read(cx).ui().file_history.clone())
        .expect("File History open");
    let (fh_fit, fh_diff) = cx.read(|cx| {
        let view = pane.read(cx);
        (view.header_fit.clone(), view.diff_pane.clone())
    });
    assert_fits(&fh_fit, &FH_CONTROLS, dimensions, &format!("{label}/fh"));
    let fh_diff = fh_diff
        .downcast::<FhDiffPane>()
        .expect("File History diff pane");
    let fh_diff_fit = cx.read(|cx| fh_diff.read(cx).fit.clone());
    assert!(
        cx.read(|cx| fh_diff.read(cx).diff.is_some()),
        "{label}: File History shows the selected entry's diff"
    );
    assert_fits(
        &fh_diff_fit,
        &FH_DIFF_CONTROLS,
        dimensions,
        &format!("{label}/fh-diff"),
    );
    drop((diff, pane, fh_diff));
    unmount(cx, app, win);
    compact
}

pub fn scenario_header_fit(cx: &mut VisualTestAppContext) {
    let restore = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path();
    let file = repo.join(LONG_PATH);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fn main() {}\n".repeat(40)).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "a file with a long path"]);

    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        theme::set_zoom(1.25);
        let compact = check(cx, repo, (1000., 720.), &format!("{locale}/1000x720@1.25"));
        assert!(
            compact,
            "{locale}: the narrow header needs its icon fallback"
        );
    }
    std::env::set_var("KAGI_LANG", "en");
    i18n::init_lang();
    theme::set_zoom(1.);
    let compact = check(cx, repo, (1920., 1080.), "en/1920x1080@1");
    assert!(!compact, "a wide header keeps its labels");
    drop(restore);
    eprintln!("[gui-e2e] PASS header_fit: diff + File History header controls inside the window (EN/JA 1000x720@1.25, labels kept at 1920)");
}
