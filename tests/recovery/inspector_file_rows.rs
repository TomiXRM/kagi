//! #1559: the Inspector file option owns its entire available row width.
use crate::keyboard_nav::keys;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::{point, px, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, i18n, list_a11y, theme, KagiApp};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    for path in ["src/nested/a.txt", "src/nested/b.txt"] {
        std::fs::write(dir.path().join(path), "first\nsecond\n").unwrap();
    }
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "Files"]);
    dir
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle, size: (f32, f32)) {
    let wanted = gpui::size(px(size.0), px(size.1));
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
        assert!(std::time::Instant::now() < deadline, "Inspector resize");
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

fn assert_file(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, path: &str) {
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().main_diff.as_ref().map(|diff| diff
            .read(cx)
            .view
            .title
            .to_string())),
        Some(path.to_owned()),
        "clicking the Inspector row right edge must open the same file diff"
    );
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    let selected: Vec<_> = ax.rows.values().filter(|(_, selected)| *selected).collect();
    assert_eq!(selected.len(), 1);
    assert!(selected[0].0.ends_with(path));
}

fn paint_edges(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    row: Bounds<Pixels>,
    ring: bool,
) {
    cx.update_window(window, |_, w, _| {
        let image = w.render_to_image().expect("native Inspector row pixels");
        let scale = w.scale_factor();
        let sample = |x: Pixels, y: Pixels| {
            image
                .get_pixel((f32::from(x) * scale) as u32, (f32::from(y) * scale) as u32)
                .0
        };
        let y = row.top() + px(4.);
        let left = sample(row.left() + px(5.), y);
        let right = sample(row.right() - px(5.), y);
        assert_eq!(
            &left[..3],
            &right[..3],
            "selection fill spans both row edges"
        );
        if ring {
            let left_ring = sample(row.left() + px(0.5), row.center().y);
            let right_ring = sample(row.right() - px(0.5), row.center().y);
            assert_eq!(
                &left_ring[..3],
                &right_ring[..3],
                "focus ring spans the full row"
            );
            assert_ne!(
                &left_ring[..3],
                &left[..3],
                "keyboard focus ring is visible"
            );
        }
    })
    .unwrap();
}

fn case(cx: &mut VisualTestAppContext, repo: &std::path::Path, size: (f32, f32), tree: bool) {
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.inspector_visible = true;
        app.inspector_tree_view = tree;
        app.sidebar.width = 240.;
        app.panel_width = 360.;
        app.bottom_panel_open = false;
        app.select(0);
        cx.notify();
    });
    draw(cx, window, size);
    for region in ["right", "name", "stat"] {
        let row = bounds(window, "inspector-file-0");
        let viewport = bounds(window, "inspector-files-scroll");
        let position = match region {
            "right" => point(viewport.right() - px(4.), row.center().y),
            "name" => bounds(window, "inspector-file-name-0").center(),
            _ => bounds(window, "inspector-file-stat-0").center(),
        };
        cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
        cx.simulate_click(window, position, gpui::Modifiers::none());
        draw(cx, window, size);
        assert_file(cx, &app, "src/nested/a.txt");
        let selected_row = bounds(window, "inspector-file-0");
        assert!((selected_row.left() - viewport.left()).abs() <= px(0.6));
        assert!(
            (selected_row.right() - viewport.right()).abs() <= px(0.6),
            "file option fills available Inspector width"
        );
        paint_edges(cx, window, selected_row, false);
        app.update(cx, |app, cx| {
            app.close_main_diff();
            cx.notify();
        });
        draw(cx, window, size);
    }
    keys(cx, window, "down space");
    draw(cx, window, size);
    assert_file(cx, &app, "src/nested/b.txt");
    let second = bounds(window, "inspector-file-1");
    paint_edges(cx, window, second, true);
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
    unmount(cx, app, window);
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let repo = fixture();
    let before = repo_fingerprint(repo.path());
    for preset in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(preset));
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                for size in [(1000., 720.), (1400., 900.)] {
                    for tree in [false, true] {
                        case(cx, repo.path(), size, tree);
                    }
                }
            }
        }
    }
    assert_eq!(before, repo_fingerprint(repo.path()));
    eprintln!("[gui-e2e] PASS inspector_file_row_hit");
}
