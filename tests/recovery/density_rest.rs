//! #1498/#1499: paint-stage typography on unchanged dense rows.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{px, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, BottomTab, KagiApp};

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
fn inspect(cx: &mut VisualTestAppContext, log: bool) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let fixture = build_fixture();
    for lang in [Lang::En, Lang::Ja] {
        for slug in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                for width in [940., 1392.] {
                    let (app, window) = mount(cx, fixture.path());
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                        if log {
                            app.bottom_panel_open = true;
                            app.bottom_tab = BottomTab::OperationLog;
                            app.bottom_panel_height = 360.;
                        }
                        cx.notify();
                    });
                    crate::pr_detail_adaptive::resize(cx, window, (width, 883.));
                    if log {
                        let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
                        panel.update(cx, |panel, cx| {
                            *panel = kagi::ui::oplog_panel::OpLogPanel::from_entries(
                                vec![crate::recovery_oplog_keyboard::entry(
                                    fixture.path(),
                                    0,
                                    1,
                                    true,
                                )]
                                .into(),
                            );
                            cx.notify();
                        });
                        drop(panel);
                    } else {
                        let mut page = crate::lists_keyboard::page();
                        page.issues.truncate(1);
                        page.issues[0].title = "Long issue 日本語 title ".repeat(30);
                        page.issues[0].comment_count = 42;
                        page.issues[0].updated_at = "2026-10-13T10:00:00Z".into();
                        KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page)));
                        app.update(cx, |app, cx| {
                            app.seed_issue_composer_for_e2e(cx);
                            app.show_issues_mode(cx);
                        });
                    }
                    draw(cx, window);
                    if !log && width < 1000. {
                        app.update(cx, |app, cx| app.show_graph_mode(cx));
                        draw(cx, window);
                        let trigger =
                            e2e::control_bounds(window.window_id(), "compact-rail-issues").unwrap();
                        cx.simulate_click(window, trigger.center(), gpui::Modifiers::none());
                        draw(cx, window);
                        assert!(
                            e2e::control_bounds(window.window_id(), "compact-sidebar-popover")
                                .is_some()
                        );
                    }
                    let names: &[&str] = if log {
                        &["oplog-time-0", "oplog-operation-0", "oplog-outcome-0"]
                    } else {
                        &["issue-secondary-1", "issue-activity-1", "issue-number-1"]
                    };
                    for name in names {
                        let paint = e2e::control_paint(window.window_id(), name)
                            .unwrap_or_else(|| panic!("missing {name}"));
                        assert!(
                            (paint.font_size - theme::scaled_px(13.)).abs() < px(0.1),
                            "{name}: scaled 13px role, actual {:?}",
                            paint.font_size
                        );
                        assert!(
                            paint.bounds.top() >= paint.mask.top()
                                && paint.bounds.bottom() <= paint.mask.bottom(),
                            "{name}: line contained in mask"
                        );
                        cx.update_window(window, |_, window, _| {
                            let mut style = window.text_style();
                            style.font_family = theme::UI_FONT.into();
                            let system = window.text_system();
                            let age = kagi_ui_core::time::relative_time(
                                kagi_ui_core::time_parse::iso_to_epoch("2026-10-13T10:00:00Z")
                                    .unwrap(),
                                kagi_ui_core::time::now_unix_secs(),
                            );
                            let text = match *name {
                                "oplog-time-0" => kagi::ui::format_hms(1700000000),
                                "oplog-operation-0" => "keyboard-entry-0".into(),
                                "oplog-outcome-0" => kagi::ui::oplog_panel::outcome_summary(
                                    &crate::recovery_oplog_keyboard::entry(
                                        fixture.path(),
                                        0,
                                        1,
                                        true,
                                    ),
                                ),
                                "issue-number-1" => "#1".into(),
                                _ => format!("{age} · {}", kagi::ui::i18n::issue_comments(42)),
                            };
                            let line = system.shape_line(
                                text.clone().into(),
                                paint.font_size,
                                &[style.to_run(text.len())],
                                None,
                            );
                            let baseline = paint.bounds.top()
                                + (paint.bounds.size.height - line.ascent - line.descent) / 2.
                                + line.ascent;
                            for (index, character) in text.char_indices() {
                                let font = line.font_id_for_index(index).unwrap();
                                let glyph = system
                                    .typographic_bounds(font, paint.font_size, character)
                                    .unwrap();
                                assert!(
                                    baseline - glyph.bottom() >= paint.mask.top()
                                        && baseline - glyph.top() <= paint.mask.bottom(),
                                    "{name}: glyph {character:?} inside mask"
                                );
                            }
                        })
                        .unwrap();
                    }
                    if !log {
                        let row =
                            e2e::control_bounds(window.window_id(), "issue-mode-card-1").unwrap();
                        let secondary =
                            e2e::control_paint(window.window_id(), "issue-secondary-1").unwrap();
                        assert!(
                            secondary.bounds.left() >= row.left()
                                && secondary.bounds.right() <= row.right()
                        );
                        let activity =
                            e2e::control_bounds(window.window_id(), "issue-activity-1").unwrap();
                        let number =
                            e2e::control_bounds(window.window_id(), "issue-number-1").unwrap();
                        assert!(
                            activity.right() <= number.left(),
                            "long title cannot displace the number"
                        );
                        assert!(number.right() <= row.right() && number.bottom() <= row.bottom());
                        app.update(cx, |app, cx| {
                            app.ui.values_mut().next().unwrap().selected_github_issue = Some(1);
                            app.set_window_active_for_e2e(true, cx);
                            cx.notify();
                        });
                        draw(cx, window);
                        let selected =
                            e2e::control_paint(window.window_id(), "issue-secondary-1").unwrap();
                        assert_eq!(
                            selected.text_color,
                            theme::theme().selection_secondary_foreground(true)
                        );
                        assert_eq!(selected.bounds.size.height, secondary.bounds.size.height);
                        assert_eq!(
                            e2e::control_bounds(window.window_id(), "issue-mode-card-1")
                                .unwrap()
                                .size
                                .height,
                            row.size.height
                        );
                    } else {
                        let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
                        let row = cx
                            .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(0).unwrap());
                        assert!((row.size.height - theme::scaled_px(44.)).abs() < px(0.6));
                        drop(panel);
                    }
                    unmount(cx, app, window);
                }
            }
        }
    }
}
pub fn oplog(cx: &mut VisualTestAppContext) {
    inspect(cx, true);
}
pub fn issues(cx: &mut VisualTestAppContext) {
    narrow_issue_metadata(cx);
    inspect(cx, false);
}

fn narrow_issue_metadata(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    crate::pr_detail_adaptive::resize(cx, window, (1392., 883.));
    let mut page = crate::lists_keyboard::page();
    page.issues.truncate(1);
    page.issues[0].number = 12345;
    page.issues[0].title = "Long issue title 日本語 ".repeat(30);
    page.issues[0].comment_count = 1234;
    page.issues[0].updated_at = "12mo ago".into();
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page)));
    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    for lang in [Lang::En, Lang::Ja] {
        for slug in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                for width in [120., 160.] {
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                        app.sidebar.width = width;
                        cx.notify();
                    });
                    draw(cx, window);
                    let row =
                        e2e::control_bounds(window.window_id(), "issue-mode-card-12345").unwrap();
                    let activity =
                        e2e::control_paint(window.window_id(), "issue-activity-12345").unwrap();
                    let number =
                        e2e::control_paint(window.window_id(), "issue-number-12345").unwrap();
                    let expected_height = theme::scaled_px(18.) + theme::scaled_px(13.) + px(16.);
                    assert!(
                        (row.size.height - expected_height).abs() <= px(1.),
                        "narrow metadata preserves the two-line card height"
                    );
                    assert!(number.bounds.right() <= row.right() && number.bounds.right() <= number.mask.right(),
                        "sidebar {width}/{zoom}: #12345 must fit inside row and paint mask: number {:?}, row {:?}, mask {:?}", number.bounds, row, number.mask);
                    assert!(
                        activity.bounds.right() <= number.bounds.left(),
                        "activity must yield width to #number"
                    );
                    assert!(
                        activity.ellipsis,
                        "narrow activity uses truncation rather than losing the issue number"
                    );
                }
            }
        }
    }
    unmount(cx, app, window);
}
