//! #1467: remaining Finder-style file/ref surfaces.
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::{px, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, theme};

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "glyphpackage"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/glyphpackage", "HEAD"],
    );
    git(fixture.path(), &["tag", "glyphpackage"]);
    std::fs::write(fixture.path().join("README.md"), "stash glyphs\n").unwrap();
    git(fixture.path(), &["stash", "push", "-qm", "glyphpackage"]);
    let linked = tempfile::tempdir().unwrap();
    let linked_path = linked.path().join("glyphpackage");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-qb",
            "glyph-worktree",
            linked_path.to_str().unwrap(),
        ],
    );
    let before = repo_fingerprint(fixture.path());
    for preset in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(preset));
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            cx.update(theme::sync_gpui_component_theme);
            let (app, window) = mount(cx, fixture.path());
            app.update(cx, |app, cx| {
                app.sidebar.pane_weights = [1, 1, 1, 1, 1];
                app.sidebar.collapsed.clear();
                cx.notify();
            });
            draw(cx, window);
            for (pane, row, label, text, height) in [
                (
                    0,
                    "sidebar-local-main",
                    "sidebar-local-name-main",
                    "main",
                    20.,
                ),
                (
                    0,
                    "sidebar-local-glyphpackage",
                    "sidebar-local-name-glyphpackage",
                    "glyphpackage",
                    20.,
                ),
                (
                    1,
                    "sidebar-remote-origin/glyphpackage",
                    "sidebar-remote-name-origin/glyphpackage",
                    "glyphpackage",
                    20.,
                ),
                (
                    3,
                    "sidebar-tag-glyphpackage",
                    "sidebar-tag-name-glyphpackage",
                    "glyphpackage",
                    20.,
                ),
                (
                    4,
                    "sidebar-stash-0",
                    "sidebar-stash-name-0",
                    "stash@{0}: On main: glyphpackage",
                    20.,
                ),
                (
                    2,
                    "sidebar-worktree-glyphpackage",
                    "sidebar-worktree-name-glyphpackage",
                    "glyphpackage",
                    24.,
                ),
            ] {
                // Enlarge the inspected pane: partial overscan rows are correctly
                // clipped by the pane and are not glyph-containment candidates.
                app.update(cx, |app, cx| {
                    app.sidebar.pane_weights = [1; 5];
                    app.sidebar.pane_weights[pane] = 1000;
                    cx.notify();
                });
                draw(cx, window);
                let bounds = e2e::control_bounds(window.window_id(), row)
                    .unwrap_or_else(|| panic!("missing {row}"));
                assert!(
                    (bounds.size.height - theme::scaled_px(height)).abs() <= px(0.6),
                    "{row}: preserved row height"
                );
                let paint = e2e::control_paint(window.window_id(), label)
                    .unwrap_or_else(|| panic!("missing {label}"));
                assert!(
                    (paint.font_size - theme::scaled_px(13.)).abs() <= px(0.1),
                    "{label}: scaled 13 font, actual {:?}",
                    paint.font_size
                );
                assert_glyphs(cx, window, text, paint);
            }
            unmount(cx, app, window);
        }
    }
    assert_eq!(before, repo_fingerprint(fixture.path()));
    crate::finder_scale_editor::inspect(cx);
    crate::finder_scale_history::inspect(cx);
    eprintln!("[gui-e2e] PASS finder_scale_rest");
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    for _ in 0..4 {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        cx.run_until_parked();
    }
}

fn assert_glyphs(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    text: &str,
    paint: e2e::ControlPaint,
) {
    cx.update_window(window, |_, window, _| {
        let mut style = window.text_style();
        style.font_family = theme::UI_FONT.into();
        style.font_fallbacks = kagi_ui_core::fonts::ui_font().fallbacks;
        style.font_weight = paint.font_weight;
        let system = window.text_system();
        let line = system.shape_line(text.into(), paint.font_size, &[style.to_run(text.len())], None);
        let baseline = paint.bounds.top() + (paint.bounds.size.height - line.ascent - line.descent) / 2. + line.ascent;
        for (index, character) in text.char_indices() {
            let font = line.font_id_for_index(index).unwrap();
            let glyph = system.typographic_bounds(font, paint.font_size, character).unwrap();
            assert!(baseline - glyph.bottom() >= paint.mask.top() && baseline - glyph.top() <= paint.mask.bottom(), "sidebar {text} glyph {character:?}: {:?}..{:?} must fit mask {:?}..{:?}, label {:?}", baseline - glyph.bottom(), baseline - glyph.top(), paint.mask.top(), paint.mask.bottom(), paint.bounds);
        }
    }).unwrap();
}
