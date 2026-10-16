//! #1559: the Inspector file option owns its entire available row width.
use crate::keyboard_nav::keys;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::{point, px, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, i18n, list_a11y, theme, KagiApp};

/// Inspector-only oracles require the independent desktop frame at every zoom.
pub(crate) fn mount_desktop(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let (app, window) = mount(cx, repo);
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    let width = f32::from(viewport.width)
        .max((theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil());
    crate::pr_detail_adaptive::resize(cx, window, (width, f32::from(viewport.height)));
    (app, window)
}

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
        list_a11y::clear_recorded_lists();
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
        assert!(
            (row.size.height - KagiApp::inspector_row_height_for_e2e()).abs() < px(0.6),
            "uniform file slots keep their scaled native height"
        );
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
                // Right-edge Inspector row geometry requires the desktop host.
                let desktop_width =
                    (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil();
                for size in [(desktop_width, 720.), (1400_f32.max(desktop_width), 900.)] {
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

// Shared with the motion and compact consumers: source indices remain the
// production commit-file indices, never the current flattened render positions.
pub(crate) fn accordion_fixture() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    for path in [
        "alpha/nested/a.txt",
        "alpha/nested/b.txt",
        "alpha/root.txt",
        "beta/nested/a.txt",
        "chain/one/two/leaf.txt",
        "z.txt",
        "Cargo.lock",
    ] {
        let file = repo.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            file,
            if path == "Cargo.lock" {
                "# This file is automatically @generated by Cargo.\nversion = 4\n"
            } else {
                "first\nsecond\n"
            },
        )
        .unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "Directory disclosure"]);
    repo
}

fn accordion_draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle, size: (f32, f32)) {
    list_a11y::clear_recorded_lists();
    draw(cx, window, size);
}

fn accordion_click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    cx.simulate_click(window, bounds(window, id).center(), gpui::Modifiers::none());
}

fn accordion_file(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, path: &str) -> String {
    let index = cx
        .read(|cx| app.read(cx).inspector_file_index_for_e2e(path))
        .unwrap();
    format!("inspector-file-{index}")
}

fn accordion_present(path: &str, present: bool) {
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    assert_eq!(
        ax.rows.values().any(|(name, _)| name.ends_with(path)),
        present,
        "directory disclosure updates the semantic file set immediately: {path}"
    );
}

fn accordion_diff(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, path: Option<&str>) {
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .main_diff
            .as_ref()
            .map(|diff| { diff.read(cx).view.title.to_string() })),
        path.map(str::to_owned),
        "directory activation must preserve the existing diff"
    );
}

fn accordion_case(cx: &mut VisualTestAppContext, repo: &std::path::Path, compact: bool) {
    let size = if compact { (900., 883.) } else { (1392., 883.) };
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.inspector_visible = true;
        app.inspector_tree_view = true;
        app.inspector_generated_expanded = false;
        app.sidebar.width = 240.;
        app.panel_width = 360.;
        app.bottom_panel_open = compact;
        app.select(0);
        app.close_main_diff();
        cx.notify();
    });
    accordion_draw(cx, window, size);
    if compact {
        accordion_click(cx, window, "compact-tab-changes");
        accordion_draw(cx, window, size);
        bounds(window, "compact-inspector-files");
    }
    let a = accordion_file(cx, &app, "alpha/nested/a.txt");
    let b = accordion_file(cx, &app, "alpha/nested/b.txt");
    let root = accordion_file(cx, &app, "alpha/root.txt");
    for path in [
        "alpha/nested/a.txt",
        "beta/nested/a.txt",
        "chain/one/two/leaf.txt",
    ] {
        accordion_present(path, true);
    }
    accordion_click(cx, window, "inspector-directory-alpha/nested");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", false);
    accordion_present("alpha/nested/b.txt", false);
    accordion_present("alpha/root.txt", true);
    accordion_present("beta/nested/a.txt", true);
    accordion_diff(cx, &app, None);
    // Pointer focus stays on the toggled directory. Down must skip both
    // hidden descendants and activate the next visible file, not a stale slot.
    keys(cx, window, "down space");
    accordion_draw(cx, window, size);
    assert_file(cx, &app, "alpha/root.txt");
    accordion_click(cx, window, "inspector-directory-alpha");
    accordion_draw(cx, window, size);
    accordion_present("alpha/root.txt", false);
    accordion_diff(cx, &app, Some("alpha/root.txt"));
    keys(cx, window, "right");
    accordion_draw(cx, window, size);
    accordion_present("alpha/root.txt", true);
    accordion_present("alpha/nested/a.txt", false);
    if !compact {
        // The full presentation round trip is covered once, on the wide
        // consumer; the compact leg checks its own real controls and focus.
        accordion_click(cx, window, "toggle-path");
        accordion_draw(cx, window, size);
        accordion_present("alpha/nested/a.txt", true);
        accordion_click(cx, window, "toggle-tree");
        accordion_draw(cx, window, size);
        accordion_present("alpha/nested/a.txt", false);
        accordion_present("alpha/root.txt", true);
    }
    // A refresh can temporarily lose the cached file read without changing SHA.
    let (row, saved_files) = app.update(cx, |app, cx| {
        let ui = app.ui_mut().unwrap();
        let row = ui.selected.unwrap();
        let files = ui.diff_caches.changed_files()[&row].clone();
        ui.diff_caches.insert_row(row, None, None, None);
        cx.notify();
        (row, files)
    });
    accordion_draw(cx, window, size);
    app.update(cx, |app, cx| {
        app.ui_mut()
            .unwrap()
            .diff_caches
            .insert_row(row, saved_files, None, None);
        cx.notify();
    });
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", false);
    accordion_present("alpha/root.txt", true);
    accordion_click(cx, window, "inspector-directory-alpha/nested");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", true);
    keys(cx, window, "left");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", false);
    keys(cx, window, "enter");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", true);
    keys(cx, window, "space");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", false);
    keys(cx, window, "right down space");
    accordion_draw(cx, window, size);
    assert_file(cx, &app, "alpha/nested/a.txt");
    accordion_click(cx, window, &b);
    accordion_draw(cx, window, size);
    assert_file(cx, &app, "alpha/nested/b.txt");
    // Selecting a leaf does not make its ancestor disclosure own the diff.
    accordion_click(cx, window, "inspector-directory-alpha");
    accordion_draw(cx, window, size);
    accordion_diff(cx, &app, Some("alpha/nested/b.txt"));
    accordion_present("alpha/nested/b.txt", false);
    accordion_click(cx, window, "inspector-directory-beta/nested");
    accordion_draw(cx, window, size);
    accordion_present("beta/nested/a.txt", false);
    accordion_click(cx, window, "inspector-directory-alpha");
    accordion_draw(cx, window, size);
    accordion_present("alpha/nested/a.txt", true);
    accordion_present("beta/nested/a.txt", false);
    bounds(window, &a);
    bounds(window, &root);
    accordion_click(cx, window, "inspector-directory-chain/one/two");
    accordion_draw(cx, window, size);
    accordion_present("chain/one/two/leaf.txt", false);
    accordion_click(cx, window, "generated-fold-header");
    accordion_draw(cx, window, size);
    accordion_present("Cargo.lock", true);
    accordion_present("beta/nested/a.txt", false);
    accordion_present("chain/one/two/leaf.txt", false);
    accordion_click(cx, window, "inspector-directory-beta/nested");
    accordion_draw(cx, window, size);
    accordion_present("beta/nested/a.txt", true);
    accordion_click(cx, window, "generated-fold-header");
    accordion_draw(cx, window, size);
    accordion_present("Cargo.lock", false);
    accordion_present("beta/nested/a.txt", true);
    accordion_diff(cx, &app, Some("alpha/nested/b.txt"));
    unmount(cx, app, window);
}

pub(crate) fn directory_accordion(cx: &mut VisualTestAppContext, compact: bool) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = crate::inspector_motion::MotionRestore(theme::reduce_motion());
    theme::set_zoom(1.);
    theme::set_reduce_motion(true);
    cx.update(theme::sync_gpui_component_theme);
    let repo = accordion_fixture();
    let before = repo_fingerprint(repo.path());
    accordion_case(cx, repo.path(), compact);
    assert_eq!(before, repo_fingerprint(repo.path()));
}

pub fn scenario_directory_accordion(cx: &mut VisualTestAppContext) {
    directory_accordion(cx, false);
}
