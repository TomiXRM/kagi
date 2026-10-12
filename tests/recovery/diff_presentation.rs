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
    let removed = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba([(x * 17) as u8, (y * 31) as u8, (x * 11 + y * 23) as u8, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(removed)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    std::fs::write(repo.join("deleted.png"), bytes.into_inner()).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "before"]);
    std::fs::write(repo.join("changed.png"), png_bytes([0, 255, 0, 255])).unwrap();
    std::fs::write(repo.join("added.png"), png_bytes([0, 0, 255, 255])).unwrap();
    std::fs::write(repo.join("unsupported.bin"), b"\0new").unwrap();
    std::fs::remove_file(repo.join("deleted.png")).unwrap();
    git(repo, &["add", "-A"]);
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
            ("deleted.png", "Deleted image", "削除された画像"),
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

pub(crate) fn assert_conflict_binary_states(cx: &mut VisualTestAppContext) {
    let restore = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("binary.bin"), b"\0base").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "binary base"]);
    git(repo, &["checkout", "-qb", "binary-incoming"]);
    std::fs::write(repo.join("binary.bin"), b"\0incoming").unwrap();
    git(repo, &["commit", "-qam", "incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("binary.bin"), b"\0current").unwrap();
    git(repo, &["commit", "-qam", "current"]);
    let repository = git2::Repository::open(repo).unwrap();
    let commit = repository
        .revparse_single("binary-incoming")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let annotated = repository.find_annotated_commit(commit.id()).unwrap();
    repository.merge(&[&annotated], None, None).unwrap();
    drop(annotated);
    drop(commit);
    drop(repository);
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        e2e::diff_presentation::clear();
        let (app, win) = mount(cx, repo);
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
        conflict.update(cx, |view, cx| {
            view.conflict_select_file(0);
            cx.notify();
        });
        frame(cx, win);
        assert_eq!(
            e2e::diff_presentation::description().as_deref(),
            Some(if locale == "en" {
                "Binary file — no text diff. Open in external editor to compare."
            } else {
                "binary ファイル — テキスト diff はありません。外部エディタで開いて比較してください。"
            }),
            "{locale}: binary conflict explanation must render"
        );
        let button = e2e::control_bounds(win.window_id(), "conflict-open-both-external").unwrap();
        let body_bottom = [
            "conflict-binary-current-body",
            "conflict-binary-incoming-body",
        ]
        .map(|name| e2e::control_bounds(win.window_id(), name).unwrap().bottom())
        .into_iter()
        .max()
        .unwrap();
        assert!(
            (f32::from(button.top() - body_bottom - theme::scaled_px(8.))).abs() < 1.,
            "{locale}: external compare must follow binary captions immediately"
        );
        drop(conflict);
        unmount(cx, app, win);
    }
    drop(restore);
}

pub fn scenario_conflict_image_zoom(cx: &mut VisualTestAppContext) {
    let restore = GlobalSettings::capture();
    let fixture = conflict_image_fixture();
    let repo = fixture.path();
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        for zoom in [1.67, 1.] {
            theme::set_zoom(zoom);
            let (app, win) = mount(cx, repo);
            app.update(cx, |app, cx| app.detect_conflict_mode(cx));
            cx.run_until_parked();
            let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
            conflict.update(cx, |view, cx| {
                view.conflict_select_file(0);
                cx.notify();
            });
            for dimensions in [(940., 660.), (1392., 883.)] {
                resize_image_frame(cx, win, dimensions);
                assert_image_geometry(win, locale, zoom, dimensions);
            }
            drop(conflict);
            unmount(cx, app, win);
        }
    }
    drop(restore);
    eprintln!("[gui-e2e] PASS conflict_image_zoom");
}

fn conflict_image_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("image.png"), png_bytes([255, 0, 0, 255])).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "image base"]);
    git(repo, &["checkout", "-qb", "image-incoming"]);
    std::fs::write(repo.join("image.png"), png_bytes([0, 255, 0, 255])).unwrap();
    git(repo, &["commit", "-qam", "incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("image.png"), png_bytes([0, 0, 255, 255])).unwrap();
    git(repo, &["commit", "-qam", "current"]);
    let repository = git2::Repository::open(repo).unwrap();
    let commit = repository
        .revparse_single("image-incoming")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let annotated = repository.find_annotated_commit(commit.id()).unwrap();
    repository.merge(&[&annotated], None, None).unwrap();
    drop(annotated);
    drop(commit);
    drop(repository);
    fixture
}

fn resize_image_frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    let wanted = gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1));
    cx.update_window(win, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(win, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "native resize failed");
    }
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, win, dimensions);
    }
}

fn assert_image_geometry(win: AnyWindowHandle, locale: &str, zoom: f32, dimensions: (f32, f32)) {
    let bounds = |name: &str| e2e::control_bounds(win.window_id(), name).unwrap();
    let pane = bounds("conflict-image-pane");
    let current = bounds("conflict-image-current");
    let incoming = bounds("conflict-image-incoming");
    for side in ["conflict-image-current", "conflict-image-incoming"] {
        let column = bounds(&format!("{side}-column"));
        crate::recovery_layout::contained(pane, column, "image column");
        crate::recovery_layout::contained(column, bounds(side), "image box");
        crate::recovery_layout::contained(
            column,
            bounds(&format!("{side}-header")),
            "image header",
        );
        crate::recovery_layout::contained(
            column,
            bounds(&format!("{side}-caption")),
            "image metadata",
        );
        assert!((f32::from(column.size.width - bounds(side).size.width)).abs() < 1.);
        assert!(
            bounds(side).size.height > gpui::px(0.),
            "image box must not collapse"
        );
        assert!(
            bounds(side).size.height <= theme::scaled_px(260.) + gpui::px(1.),
            "image box exceeds its scaled height cap"
        );
        let caption = e2e::diff_presentation::image_caption(side).unwrap();
        assert!(caption.contains(i18n::Msg::ImageFileSize.t()));
        assert!(caption.contains(i18n::Msg::ImageDimensions.t()));
        assert!(caption.contains("2 × 2 px"), "{caption}");
    }
    assert_eq!(
        e2e::diff_presentation::conflict_scroll()
            .unwrap()
            .max_offset()
            .x,
        gpui::px(0.),
        "{locale}/{zoom}/{dimensions:?}: horizontal scroll extent must be zero"
    );
    assert_eq!(
        e2e::diff_presentation::conflict_scroll()
            .unwrap()
            .max_offset()
            .y,
        gpui::px(0.),
        "{locale}/{zoom}/{dimensions:?}: vertical scroll extent must be zero"
    );
    for image in [current, incoming] {
        assert!(
            image.left() >= pane.left() && image.right() <= pane.right(),
            "{locale}/{zoom}/{dimensions:?}: image overflows pane: {image:?} / {pane:?}"
        );
        assert!(image.top() >= pane.top() && image.bottom() <= pane.bottom());
    }
    assert!((f32::from(current.size.width - incoming.size.width)).abs() < 1.);
    assert!(
        (f32::from(incoming.left() - current.right() - theme::scaled_px(12.))).abs() < 1.,
        "{locale}/{zoom}/{dimensions:?}: image gap must use scaled units"
    );
}
