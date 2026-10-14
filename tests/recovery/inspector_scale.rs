//! #1464: Inspector density matches sidebar rows without changing pane budgets.
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::{px, AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme, KagiApp};

fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    git(fixture.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir(fixture.path().join("src")).unwrap();
    for ix in 0..105 {
        std::fs::write(
            fixture.path().join(format!("src/package-{ix:03}.json")),
            "file\n",
        )
        .unwrap();
    }
    std::fs::write(fixture.path().join("Cargo.lock"), "version = 3\n").unwrap();
    git(fixture.path(), &["add", "."]);
    git(fixture.path(), &["commit", "-qm", "Inspector scale"]);
    fixture
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle, width: f32, height: f32) {
    let wanted = gpui::size(px(width), px(height));
    cx.update_window(window, |_, w, _| w.resize(wanted))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(window, |_, w, _| w.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native Inspector viewport resized"
        );
    }
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(window, |_, w, cx| {
            w.refresh();
            w.draw(cx).clear();
        })
        .unwrap();
    }
}

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name).unwrap_or_else(|| panic!("missing {name}"))
}

fn assert_row(window: AnyWindowHandle, name: &str, zoom: f32) {
    let height = bounds(window, name).size.height;
    assert!(
        (height - theme::scaled_px(20.)).abs() <= px(0.6),
        "{name}: {height:?}"
    );
    if zoom == 1.67 {
        assert!(height >= px(20.), "{name}: high-zoom logical hit target");
    }
}

fn inspect_rows(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    width: f32,
    height: f32,
    zoom: f32,
) {
    for name in ["inspector-file-badge-1", "inspector-file-name-1"] {
        let paint = e2e::control_paint(window.window_id(), name).expect("painted file text");
        assert!(
            (paint.font_size - theme::scaled_px(13.)).abs() <= px(0.1),
            "{name}: Finder-inspired 13-reference-pixel tree font"
        );
        let row = bounds(window, "inspector-file-1");
        assert!(
            paint.bounds.top() >= row.top() && paint.bounds.bottom() <= row.bottom(),
            "{name}: fits focus frame"
        );
    }
    let name = e2e::control_paint(window.window_id(), "inspector-file-name-1").unwrap();
    cx.update_window(window, |_, window, _| {
        let text = "package-000.json";
        let mut style = window.text_style();
        style.font_family = theme::UI_FONT.into();
        style.font_weight = name.font_weight;
        let text_system = window.text_system();
        let line = text_system.shape_line(
            text.into(),
            name.font_size,
            &[style.to_run(text.len())],
            None,
        );
        let baseline = name.bounds.top()
            + (name.bounds.size.height - line.ascent - line.descent) / 2.
            + line.ascent;
        for (index, character) in text.char_indices() {
            let font = line.font_id_for_index(index).expect("shaped filename font");
            let glyph = text_system
                .typographic_bounds(font, name.font_size, character)
                .expect("native filename glyph bounds");
            // CoreText glyph coordinates are baseline-relative and positive up.
            let top = baseline - glyph.bottom();
            let bottom = baseline - glyph.top();
            assert!(
                top >= name.mask.top() && bottom <= name.mask.bottom(),
                "filename glyph {character:?}, including descenders, must fit its mask: \
                 glyph={top:?}..{bottom:?}, mask={:?}..{:?}",
                name.mask.top(),
                name.mask.bottom()
            );
        }
    })
    .unwrap();
    assert!(
        (bounds(window, "inspector-file-badge-1").size.width - theme::scaled_px(16.)).abs()
            <= px(0.6),
        "Finder-inspired 16-reference-pixel badge column"
    );
    app.update(cx, |app, cx| {
        app.inspector_tree_view = true;
        cx.notify();
    });
    draw(cx, window, width, height);
    assert_row(window, "inspector-dir-0", zoom);
    for _ in 0..80 {
        crate::keyboard_nav::keys(cx, window, "tab");
        draw(cx, window, width, height);
        if cx
            .update_window(window, |_, w, cx| {
                app.read(cx)
                    .list_row_focused_for_e2e("inspector-files-scroll", w)
                    .is_some()
            })
            .unwrap()
        {
            break;
        }
    }
    e2e::clear_control_bounds(window.window_id(), "inspector-files-more");
    crate::keyboard_nav::keys(cx, window, "end");
    draw(cx, window, width, height);
    for name in ["generated-fold-header", "inspector-files-more"] {
        assert_row(window, name, zoom);
        let row = bounds(window, name);
        let viewport = bounds(window, "inspector-files-scroll");
        assert!(
            row.top() >= viewport.top() && row.bottom() <= viewport.bottom(),
            "{name}: End reveal"
        );
    }
    crate::keyboard_nav::keys(cx, window, "right down");
    draw(cx, window, width, height);
    assert_row(window, "inspector-file-0", zoom);
    crate::keyboard_nav::keys(cx, window, "enter");
    draw(cx, window, width, height);
    assert!(
        cx.read(|cx| app.read(cx).ui().main_diff.is_some()),
        "exact-file keyboard activation"
    );
    assert_eq!(
        cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
        (240., 360.),
        "fit preserves preferred pane widths"
    );
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let motion = theme::reduce_motion();
    theme::set_reduce_motion(true);
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    let mut measurements = Vec::new();
    for preset in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(preset));
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                // The desktop Inspector starts at 1000px; 940px now draws compact Changes.
                for (width, height) in [(1392., 883.), (1000., 720.)] {
                    let (app, window) = mount(cx, fixture.path());
                    app.update(cx, |app, cx| {
                        app.inspector_visible = true;
                        app.inspector_tree_view = false;
                        app.sidebar.width = 240.;
                        app.panel_width = 360.;
                        app.bottom_panel_open = false;
                        app.select(0);
                        cx.notify();
                    });
                    draw(cx, window, width, height);
                    let first = bounds(window, "inspector-file-1");
                    let header = bounds(window, "inspector-header");
                    let offset = f32::from(first.top() - header.top());
                    eprintln!("[gui-e2e] inspector_scale {preset} {lang:?} {zoom} {width}x{height}: row={:?}, first_y={offset}, estimate={:?}", first.size.height, KagiApp::inspector_row_height_for_e2e());
                    measurements.push((zoom, width, offset, first.size.height));
                    assert_row(window, "inspector-file-1", zoom);
                    inspect_rows(cx, &app, window, width, height, zoom);
                    unmount(cx, app, window);
                }
            }
        }
    }
    theme::set_reduce_motion(motion);
    assert_eq!(before, repo_fingerprint(fixture.path()));
    theme::set_zoom(1.);
    assert_eq!(
        KagiApp::inspector_row_height_for_e2e(),
        theme::scaled_px(20.),
        "Inspector uniform extent must match the sidebar's 20 reference pixels (old: 24)"
    );
    for (zoom, width, offset, height) in measurements {
        theme::set_zoom(zoom);
        assert!(
            (height - theme::scaled_px(20.)).abs() <= px(0.1),
            "Inspector row height at {width}/{zoom}: {height:?}"
        );
        if zoom == 1.67 {
            assert!(
                height >= px(20.),
                "high-zoom hit target must remain at least 20 logical pixels"
            );
        }
        if width == 1000. {
            // Native 1000×720 density ceilings, identical across languages/themes.
            // Allow rows to move up; unrelated chrome improvements must not fail this oracle.
            let expected = if zoom == 1. { 340. } else { 433.5 };
            assert!(
                offset <= expected + 0.6,
                "desktop-breakpoint density must not regress: {offset} vs {expected} at {width}/{zoom}"
            );
        } else {
            // Frozen pre-density measurements at the unchanged 1392×883 viewport.
            let old_offset = if zoom == 1. { 416.5 } else { 487.5 };
            assert!(
                offset < old_offset - 1.,
                "first row must move up: {offset} vs old {old_offset} at {width}/{zoom}"
            );
        }
    }
    eprintln!("[gui-e2e] PASS inspector_scale");
}
