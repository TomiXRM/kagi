//! #1467: native File History table density, text masks and keyboard reveal.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::{px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{i18n, theme, KagiApp};
use kagi_ui_core::commit_row::commit_row_model;
use kagi_ui_file_history::{
    clear_table_row_paints, table_row_paint, FileHistoryView, TableRowPaint,
};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    for _ in 0..3 {
        // An offscreen row must not pass using geometry from an earlier frame.
        clear_table_row_paints(window.window_id());
        crate::recovery_layout::draw(cx, window, (1440., 900.));
        cx.run_until_parked();
    }
}

fn paint(window: AnyWindowHandle, ix: usize, part: &'static str) -> TableRowPaint {
    table_row_paint(window.window_id(), ix, part)
        .unwrap_or_else(|| panic!("File History row {ix} {part} was not painted"))
}

fn inspect_text(
    cx: &mut VisualTestAppContext,
    pane: &Entity<FileHistoryView>,
    window: AnyWindowHandle,
    ix: usize,
) {
    let row = paint(window, ix, "row");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let model = cx.read(|cx| {
        commit_row_model(
            &pane.read(cx).data.history.as_ref().unwrap().entries[ix],
            now,
        )
    });
    for (part, text) in [
        ("badge", model.badge.to_string()),
        ("subject", model.subject.to_string()),
        ("author", model.author.to_string()),
        ("date", model.date_relative.to_string()),
        ("stat", model.stat.to_string()),
        ("hash", model.short_hash.to_string()),
    ] {
        let column = paint(window, ix, part);
        assert!(
            (column.font_size - theme::scaled_px(13.)).abs() <= px(0.1),
            "row {ix} {part}: Finder table font, actual {:?}",
            column.font_size
        );
        assert!(
            (column.line_height - (column.font_size * 1.2).round()).abs() <= px(0.1),
            "row {ix} {part}: 1.2 leading, actual {:?}",
            column.line_height
        );
        assert!(
            column.bounds.top() >= row.bounds.top()
                && column.bounds.bottom() <= row.bounds.bottom(),
            "row {ix} {part}: text box fits the focus frame"
        );
        if part == "badge" {
            assert!(
                (column.bounds.size.width - theme::scaled_px(16.)).abs() <= px(0.6),
                "row {ix}: Finder badge column, actual {:?}",
                column.bounds.size.width
            );
        }
        if text.is_empty() {
            continue;
        }
        cx.update_window(window, |_, window, _| {
            let mut style = window.text_style();
            style.font_family = theme::UI_FONT.into();
            style.font_fallbacks = kagi_ui_core::fonts::ui_font().fallbacks;
            style.font_weight = column.font_weight;
            let text_system = window.text_system();
            let line = text_system.shape_line(
                text.clone().into(),
                column.font_size,
                &[style.to_run(text.len())],
                None,
            );
            let baseline = column.bounds.top()
                + (column.bounds.size.height - line.ascent - line.descent) / 2.
                + line.ascent;
            for (index, character) in text.char_indices() {
                let font = line.font_id_for_index(index).expect("shaped table font");
                let glyph = text_system
                    .typographic_bounds(font, column.font_size, character)
                    .expect("native table glyph bounds");
                // CoreText bounds are baseline-relative and positive up, as in
                // inspector_scale. This checks ink, not merely a clipped box.
                let top = baseline - glyph.bottom();
                let bottom = baseline - glyph.top();
                assert!(
                    top >= column.mask.top() && bottom <= column.mask.bottom(),
                    "row {ix} {part} glyph {character:?}, including descenders, escapes mask: \
                     glyph={top:?}..{bottom:?}, mask={:?}..{:?}",
                    column.mask.top(),
                    column.mask.bottom()
                );
            }
        })
        .unwrap();
    }
}

fn inspect_navigation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    pane: &Entity<FileHistoryView>,
    window: AnyWindowHandle,
) {
    // Use the same production header/RowFocus boundary and key delivery as
    // file_history_focus_return; never install an independent focus owner.
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
    })
    .unwrap();
    for _ in 0..80 {
        keys(cx, window, "tab");
        draw(cx, window);
        if cx
            .update_window(window, |_, window, cx| {
                pane.read(cx).header_focused_for_e2e(0, window, cx)
            })
            .unwrap()
        {
            break;
        }
    }
    assert!(
        cx.update_window(window, |_, window, cx| {
            pane.read(cx).header_focused_for_e2e(0, window, cx)
        })
        .unwrap(),
        "Tab reaches File History Back"
    );
    keys(cx, window, "shift-tab down");
    draw(cx, window);
    assert_eq!(cx.read(|cx| pane.read(cx).data.selected), 1);
    keys(cx, window, "end");
    draw(cx, window);
    let last = cx.read(|cx| pane.read(cx).data.history.as_ref().unwrap().entries.len() - 1);
    assert_eq!(cx.read(|cx| pane.read(cx).data.selected), last);
    assert!(cx.read(|cx| pane.read(cx).last_built_range.contains(&last)));
    let row = paint(window, last, "row");
    assert!(
        row.bounds.top() >= row.mask.top() && row.bounds.bottom() <= row.mask.bottom(),
        "End reveals the complete selected focus frame"
    );
    inspect_text(cx, pane, window, last);
    keys(cx, window, "shift-tab");
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            pane.read(cx).header_focused_for_e2e(4, window, cx)
        })
        .unwrap(),
        "Shift-Tab from the row reaches Follow"
    );
    keys(cx, window, "tab up");
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| pane.read(cx).data.selected),
        last - 1,
        "Tab returns to the remembered RowFocus row"
    );
    keys(cx, window, "home");
    draw(cx, window);
    assert_eq!(cx.read(|cx| pane.read(cx).data.selected), 0);
    let row = paint(window, 0, "row");
    assert!(
        row.bounds.top() >= row.mask.top() && row.bounds.bottom() <= row.mask.bottom(),
        "Home reveals the complete WIP focus frame"
    );
}

pub fn inspect(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    // Extend the existing file_history_ README fixture with real commits. Both
    // the subject and author deliberately include every common descender.
    for ix in 0..32 {
        std::fs::write(
            fixture.path().join("README.md"),
            format!("# fixture\ngypqj {ix}\n"),
        )
        .unwrap();
        git(
            fixture.path(),
            &[
                "commit",
                "-qam",
                &format!("gypqj history {ix:02}"),
                "--author",
                "gypqj <gypqj@example.invalid>",
            ],
        );
    }
    std::fs::write(fixture.path().join("README.md"), "# fixture\nWIP gypqj\n").unwrap();
    let before = repo_fingerprint(fixture.path());
    for preset in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(preset));
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                let (app, window) = mount(cx, fixture.path());
                app.update(cx, |app, cx| {
                    app.open_file_history("README.md".into(), None, cx)
                });
                draw(cx, window);
                let pane = cx.read(|cx| app.read(cx).ui().file_history.clone().unwrap());
                let (entries, extent, viewport, built) = cx.read(|cx| {
                    let view = pane.read(cx);
                    let state = view.list_scroll.0.borrow();
                    let measured = state
                        .last_item_size
                        .expect("native File History list mounted");
                    (
                        view.data.history.as_ref().unwrap().entries.len(),
                        measured.contents.height,
                        measured.item.height,
                        view.last_built_range.clone(),
                    )
                });
                assert_eq!(entries, 35, "WIP plus all real README commits loaded");
                let expected = theme::scaled_px(20.);
                assert!((extent / entries as f32 - expected).abs() <= px(0.1));
                assert!(extent > viewport, "fixture exercises virtualized reveal");
                assert!(built.len() < entries, "File History remains virtualized");
                for ix in [0, 1] {
                    let pitch = paint(window, ix + 1, "row").bounds.top()
                        - paint(window, ix, "row").bounds.top();
                    assert!(
                        (pitch - expected).abs() <= px(0.6),
                        "{preset} {lang:?} {zoom}: row {ix} actual pitch {pitch:?}"
                    );
                    inspect_text(cx, &pane, window, ix);
                }
                inspect_navigation(cx, &app, &pane, window);
                clear_table_row_paints(window.window_id());
                drop(pane);
                unmount(cx, app, window);
            }
        }
    }
    assert_eq!(
        before,
        repo_fingerprint(fixture.path()),
        "scale inspection is read-only"
    );
}
