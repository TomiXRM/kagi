//! Rendered diff mode and binary-state regression coverage (#1235/#1236).
use crate::diff_highlight::{commit_image_index, png_bytes};
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{AnyWindowHandle, Modifiers, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};

fn frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, win, (1440., 900.));
    }
}

pub fn scenario_diff_mode_segment(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let restore = GlobalSettings::capture();
    theme::set_diff_split(false);
    let fixture = build_fixture();
    let (app, win) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    frame(cx, win);
    let diff = cx.read(|cx| app.read(cx).ui().main_diff.clone()).unwrap();
    let fit = cx.read(|cx| diff.read(cx).fit.clone());
    for (target, split) in [("diff-mode-split", true), ("diff-mode-unified", false)] {
        let bounds = fit
            .control_bounds(target)
            .expect("both diff mode segments must render");
        let selected = if split {
            "diff-mode-unified"
        } else {
            "diff-mode-split"
        };
        let probe = e2e::diff_presentation::mode(selected).unwrap();
        assert_eq!(probe.0, gpui::Role::Button);
        assert!(probe.2, "current mode must be selected");
        cx.simulate_mouse_move(win, bounds.center(), None, Modifiers::none());
        cx.simulate_click(win, bounds.center(), Modifiers::none());
        frame(cx, win);
        assert_eq!(theme::diff_split(), split);
        let probe = e2e::diff_presentation::mode(target).unwrap();
        assert_eq!(probe.0, gpui::Role::Button);
        assert_eq!(probe.1, if split { "Split" } else { "Unified" });
        assert!(probe.2);
        assert!(!e2e::diff_presentation::mode(selected).unwrap().2);
    }
    drop(diff);
    unmount(cx, app, win);
    drop(restore);
    eprintln!("[gui-e2e] PASS diff_mode_segment");
}

pub fn scenario_diff_binary_states(cx: &mut VisualTestAppContext) {
    let restore = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("changed.png"), png_bytes([255, 0, 0, 255])).unwrap();
    std::fs::write(repo.join("unsupported.bin"), b"\0old").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "before"]);
    std::fs::write(repo.join("changed.png"), png_bytes([0, 255, 0, 255])).unwrap();
    std::fs::write(repo.join("added.png"), png_bytes([0, 0, 255, 255])).unwrap();
    std::fs::write(repo.join("unsupported.bin"), b"\0new").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "after"]);
    let (app, win) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, win);
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        for (path, en, ja) in [
            ("unsupported.bin", "Binary file — no text diff. Open in external editor to compare.", "binary ファイル — テキスト diff はありません。外部エディタで開いて比較してください。"),
            ("added.png", "Added image (no previous version)", "追加された画像（以前のバージョンはありません）"),
            ("changed.png", "Modified image: before → after", "変更された画像：変更前 → 変更後"),
        ] {
            e2e::diff_presentation::clear();
            let index = commit_image_index(cx, &app, path);
            app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
            frame(cx, win);
            assert_eq!(e2e::diff_presentation::description().as_deref(), Some(if locale == "en" { en } else { ja }), "{locale}/{path}: explanation must render");
        }
    }
    unmount(cx, app, win);
    drop(restore);
    eprintln!("[gui-e2e] PASS diff_binary_states");
}
