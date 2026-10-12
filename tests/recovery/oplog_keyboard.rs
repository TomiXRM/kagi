//! Renderer attributes, real activation, entry identity and unchanged geometry.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, Modifiers, Role, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::Lang,
    list_a11y,
    oplog_panel::{self, keyboard, OpLogPanel},
    theme, BottomTab,
};
use kagi_git::{
    oplog::{OpLogEntry, OpOutcome},
    ops::StateSummary,
};

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn press(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
}
fn entry(repo: &std::path::Path, i: usize, variant: usize, long: bool) -> OpLogEntry {
    let state = StateSummary {
        head: "main".into(),
        dirty: "clean".into(),
    };
    let detail = if long {
        "complete detail {} ".repeat(600)
    } else {
        "complete detail {}".into()
    };
    let outcome = match variant % 3 {
        0 => OpOutcome::Success {
            after: StateSummary {
                head: detail,
                dirty: "clean".into(),
            },
        },
        1 => OpOutcome::Failed { error: detail },
        _ => OpOutcome::Refused {
            blockers: vec![detail, "second reason".into()],
        },
    };
    let mut entry = OpLogEntry::new(
        format!("keyboard-entry-{i}"),
        repo.to_str().unwrap(),
        state,
        outcome,
    );
    entry.id = i as u64 + 10;
    entry.timestamp = 1700000000 + i as i64;
    // Keep recorded ref detail, without starting estimated reflog I/O per cell.
    entry.ref_moves = Some(Vec::new());
    entry
}
fn expanded(cx: &VisualTestAppContext, panel: &Entity<OpLogPanel>) -> Option<usize> {
    cx.read(|cx| panel.read(cx).expanded())
}
fn focused(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
) -> Option<usize> {
    cx.update_window(window, |_, window, cx| {
        panel.read(cx).focused_entry_for_e2e(window)
    })
    .unwrap()
}
fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap()
}

fn activate(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
) -> gpui::Pixels {
    let bounds = cx.read(|cx| panel.read(cx).scroll_handle().bounds_for_item(0).unwrap());
    assert!(
        (f32::from(bounds.size.height) - theme::scaled(22.)).abs() < 1.,
        "collapsed height unchanged"
    );
    cx.simulate_click(
        window,
        gpui::point(
            bounds.origin.x + gpui::px(40.),
            bounds.origin.y + gpui::px(8.),
        ),
        Modifiers::none(),
    );
    paint(cx, window);
    assert_eq!(expanded(cx, panel), Some(0));
    assert_eq!(
        focused(cx, panel, window),
        Some(0),
        "pointer selection keeps the summary's keyboard handle"
    );
    press(cx, window, "enter");
    paint(cx, window);
    assert_eq!(
        expanded(cx, panel),
        None,
        "Enter on the focused Operation Log summary collapses that entry"
    );
    assert_eq!(focused(cx, panel, window), Some(0));
    bounds.size.height
}
fn attributes(count: usize) {
    let list = list_a11y::recorded_list("oplog-list").unwrap();
    assert_eq!(list.role, Some(Role::List));
    assert_eq!(list.label, kagi::ui::i18n::Msg::BottomTabOperationLog.t());
    let row = keyboard::recorded_disclosure(0).unwrap();
    assert_eq!(
        (row.role, row.expanded, row.position, row.size),
        (Role::Button, false, 1, count)
    );
    assert!(row.name.contains("#10") && row.name.contains("keyboard-entry-0"));
    let prefix = if kagi::ui::i18n::lang() == Lang::Ja {
        "操作エントリ"
    } else {
        "Operation entry"
    };
    assert!(row.name.contains(prefix));
    let copy = keyboard::recorded_copy(0).unwrap();
    assert_eq!(copy.0, Role::Button);
    assert!(copy.1.contains("keyboard-entry-0") && copy.1.contains("#10"));
}
fn geometry(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
    count: usize,
    collapsed: gpui::Pixels,
) {
    let scroll = cx.read(|cx| panel.read(cx).scroll_handle());
    let opened = scroll.bounds_for_item(0).unwrap();
    assert!(opened.size.height > collapsed);
    let copy = e2e::control_bounds(window.window_id(), "oplog-copy-0").unwrap();
    assert!((f32::from(copy.size.height) - theme::scaled(19.)).abs() < 1.);
    if count > 1 {
        scroll.scroll_to_reveal_item(1);
        paint(cx, window);
        let first = scroll
            .bounds_for_item(0)
            .expect("expanded row tail is drawn");
        let next = scroll.bounds_for_item(1).expect("next row is drawn");
        assert!(
            next.top() >= first.bottom() - gpui::px(1.),
            "expanded detail never overlaps its neighbour"
        );
        scroll.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: gpui::px(0.),
        });
        paint(cx, window);
    }
}
fn copy_full(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
    expected: &str,
) {
    // The expanded row's Copy Button follows the one roving summary Tab stop.
    press(cx, window, "tab");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before-copy".into()));
    press(cx, window, "enter");
    paint(cx, window);
    assert_eq!(
        clipboard(cx),
        expected,
        "keyboard Copy keeps every detail byte"
    );
    assert_eq!(
        expanded(cx, panel),
        Some(0),
        "Copy does not activate the disclosure"
    );
    press(cx, window, "shift-tab");
    paint(cx, window);
    assert_eq!(focused(cx, panel, window), Some(0));
    press(cx, window, "space");
    paint(cx, window);
    assert_eq!(expanded(cx, panel), None);
}
fn navigate(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
    count: usize,
) {
    if count == 1 {
        return;
    }
    press(cx, window, "down");
    paint(cx, window);
    assert_eq!(focused(cx, panel, window), Some(1));
    assert_eq!(
        expanded(cx, panel),
        None,
        "arrows do not expand neighbouring entries"
    );
    press(cx, window, "enter");
    paint(cx, window);
    assert_eq!(expanded(cx, panel), Some(1));
    press(cx, window, "enter");
    press(cx, window, "end");
    paint(cx, window);
    assert_eq!(focused(cx, panel, window), Some(count - 1));
    let last = keyboard::recorded_disclosure(count - 1).unwrap();
    assert_eq!((last.position, last.size), (count, count));
    press(cx, window, "home");
    paint(cx, window);
    assert_eq!(focused(cx, panel, window), Some(0));
}
fn refresh_copy(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
    original: OpLogEntry,
) {
    press(cx, window, "enter");
    paint(cx, window);
    press(cx, window, "tab");
    let expected = oplog_panel::entry_clipboard_text(&original);
    let mut neighbour = original;
    neighbour.id = 9999;
    neighbour.op = "new-neighbour".into();
    // Deliberately do not paint: the drawn action still belongs to the old row.
    panel.update(cx, |panel, cx| {
        panel.push(neighbour);
        cx.notify();
    });
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
        "before-refresh-copy".into(),
    ));
    press(cx, window, "enter");
    paint(cx, window);
    assert_eq!(
        clipboard(cx),
        expected,
        "refresh never copies the neighbour now at the old index"
    );
    assert_eq!(expanded(cx, panel), Some(1));
    press(cx, window, "shift-tab");
    paint(cx, window);
    assert_eq!(
        focused(cx, panel, window),
        Some(1),
        "stable summary handle follows entry identity"
    );
}
fn exercise(
    cx: &mut VisualTestAppContext,
    panel: &Entity<OpLogPanel>,
    window: AnyWindowHandle,
    count: usize,
) {
    let collapsed = activate(cx, panel, window);
    attributes(count);
    press(cx, window, "space");
    paint(cx, window);
    assert_eq!(expanded(cx, panel), Some(0));
    assert!(keyboard::recorded_disclosure(0).unwrap().expanded);
    geometry(cx, panel, window, count, collapsed);
    let original = cx.read(|cx| panel.read(cx).entries()[0].clone());
    copy_full(
        cx,
        panel,
        window,
        &oplog_panel::entry_clipboard_text(&original),
    );
    navigate(cx, panel, window, count);
    refresh_copy(cx, panel, window, original);
}

fn render_case(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    options: (Lang, f32, &str, (f32, f32)),
    count: usize,
) {
    let (lang, zoom, slug, dimensions) = options;
    let (app, window) = mount(cx, repo);
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_theme(slug, cx);
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::OperationLog;
        app.bottom_panel_height = 360.;
        cx.notify();
    });
    cx.update_window(window, |_, window, _| {
        window.resize(gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1)))
    })
    .unwrap();
    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
    for variant in 0..3 {
        for long in [false, true] {
            let entries = (0..count)
                .map(|i| entry(repo, i, variant + i, long))
                .collect();
            panel.update(cx, |panel, cx| {
                *panel = OpLogPanel::from_entries(entries);
                cx.notify();
            });
            paint(cx, window);
            exercise(cx, &panel, window, count);
        }
    }
    drop(panel);
    unmount(cx, app, window);
}
fn matrix(cx: &mut VisualTestAppContext, count: usize) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for slug in ["apple-light", "apple-dark"] {
                for dimensions in [(940., 660.), (1392., 883.)] {
                    render_case(cx, fixture.path(), (lang, zoom, slug, dimensions), count);
                }
            }
        }
    }
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
}

pub fn scenario_oplog_keyboard_single(cx: &mut VisualTestAppContext) {
    matrix(cx, 1);
}

pub fn scenario_oplog_keyboard_many(cx: &mut VisualTestAppContext) {
    matrix(cx, 200);
}

pub fn scenario_oplog_keyboard_refresh_tail(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::OperationLog;
        app.bottom_panel_height = 360.;
        cx.notify();
    });
    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
    for copy in [false, true] {
        panel.update(cx, |panel, cx| {
            *panel = OpLogPanel::from_entries(
                (0..50)
                    .map(|i| entry(fixture.path(), i, 1, false))
                    .collect(),
            );
            cx.notify();
        });
        paint(cx, window);
        activate(cx, &panel, window);
        press(cx, window, "end");
        paint(cx, window);
        assert_eq!(focused(cx, &panel, window), Some(49));
        assert!(cx.read(|cx| panel.read(cx).scroll_handle().logical_scroll_top().item_ix) > 0);
        let selected = cx.read(|cx| panel.read(cx).entries()[49].clone());
        if copy {
            press(cx, window, "enter");
            paint(cx, window);
            press(cx, window, "tab");
            paint(cx, window);
        }
        let held = cx
            .update_window(window, |_, window, cx| window.focused(cx).unwrap())
            .unwrap();
        panel.update(cx, |panel, cx| {
            panel.push(entry(fixture.path(), 999, 0, false));
            cx.notify();
        });
        paint(cx, window);
        paint(cx, window);
        let after = cx
            .update_window(window, |_, window, cx| window.focused(cx).unwrap())
            .unwrap();
        assert_eq!(
            after, held,
            "a receipt preserves the offscreen-tail entry's summary/Copy focus"
        );
        if copy {
            press(cx, window, "enter");
            assert_eq!(clipboard(cx), oplog_panel::entry_clipboard_text(&selected));
            assert_eq!(expanded(cx, &panel), Some(50));
            press(cx, window, "shift-tab");
            paint(cx, window);
        } else {
            press(cx, window, "enter");
            paint(cx, window);
            assert_eq!(expanded(cx, &panel), Some(50));
        }
        assert_eq!(focused(cx, &panel, window), Some(50));
        assert_eq!(cx.read(|cx| panel.read(cx).entries()[50].id), selected.id);
    }
    drop(panel);
    unmount(cx, app, window);
}

pub fn scenario_oplog_keyboard_refresh_top(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::OperationLog;
        app.bottom_panel_height = 360.;
        cx.notify();
    });
    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
    for variant in [1, 0] {
        panel.update(cx, |panel, cx| {
            *panel = OpLogPanel::from_entries(
                (0..50)
                    .map(|i| entry(fixture.path(), i, 0, false))
                    .collect(),
            );
            cx.notify();
        });
        paint(cx, window);
        let scroll = cx.read(|cx| panel.read(cx).scroll_handle());
        let first = scroll.bounds_for_item(0).unwrap();
        for delta in [-200., 10000.] {
            cx.simulate_event(
                window,
                gpui::ScrollWheelEvent {
                    position: first.center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
            paint(cx, window);
            if delta < 0. {
                assert!(
                    scroll.logical_scroll_top().item_ix > 0,
                    "wheel scrolled the receipts"
                );
            }
        }
        let top = scroll.logical_scroll_top();
        assert_eq!(top.item_ix, 0);
        assert_eq!(top.offset_in_item, gpui::px(0.));
        panel.update(cx, |panel, cx| {
            panel.push(entry(fixture.path(), 999, variant, false));
            cx.notify();
        });
        paint(cx, window);
        paint(cx, window);
        let latest = scroll.bounds_for_item(0).expect("latest receipt is drawn");
        assert!(
            latest.top() >= first.top() - gpui::px(1.),
            "at the top, a new receipt is inside the viewport: {latest:?} vs {first:?}"
        );
        assert_eq!(scroll.logical_scroll_top().item_ix, 0);
    }
    drop(panel);
    unmount(cx, app, window);
}

fn copy_pixels(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    bounds: gpui::Bounds<gpui::Pixels>,
) -> ([u8; 3], i32) {
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native Copy focus pixels");
        let scale = window.scale_factor();
        let pixel = image
            .get_pixel(
                (f32::from(bounds.right() - gpui::px(1.)) * scale) as u32,
                (f32::from(bounds.center().y) * scale) as u32,
            )
            .0;
        let color = theme::theme().text_sub;
        let foreground = [
            (color >> 16) as i32,
            ((color >> 8) & 255) as i32,
            (color & 255) as i32,
        ];
        let mut closest = i32::MAX;
        // The measure wrapper includes the leading margin; the trailing 19px
        // target is the button, and its central inset contains only the glyph.
        for x in ((f32::from(bounds.right()) - theme::scaled(19.) + 3.) * scale) as u32
            ..((f32::from(bounds.right()) - 3.) * scale) as u32
        {
            for y in ((f32::from(bounds.top()) + 3.) * scale) as u32
                ..((f32::from(bounds.bottom()) - 3.) * scale) as u32
            {
                let rgba = image.get_pixel(x, y).0;
                let distance = (0..3).map(|i| (rgba[i] as i32 - foreground[i]).abs()).sum();
                closest = closest.min(distance);
            }
        }
        ([pixel[0], pixel[1], pixel[2]], closest)
    })
    .unwrap()
}

pub fn scenario_oplog_keyboard_copy_focus(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    for slug in ["apple-light", "apple-dark"] {
        for zoom in [1., 1.67] {
            let (app, window) = mount(cx, fixture.path());
            theme::set_zoom(zoom);
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                app.bottom_panel_open = true;
                app.bottom_tab = BottomTab::OperationLog;
                app.bottom_panel_height = 360.;
                cx.notify();
            });
            let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
            panel.update(cx, |panel, cx| {
                *panel = OpLogPanel::from_entries([entry(fixture.path(), 0, 1, false)].into());
                cx.notify();
            });
            paint(cx, window);
            activate(cx, &panel, window);
            press(cx, window, "space");
            paint(cx, window);
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(0.), gpui::px(0.)),
                None,
                Modifiers::none(),
            );
            paint(cx, window);
            let bounds = e2e::control_bounds(window.window_id(), "oplog-copy-0").unwrap();
            let rest = copy_pixels(cx, window, bounds);
            press(cx, window, "tab");
            paint(cx, window);
            assert_eq!(
                e2e::control_bounds(window.window_id(), "oplog-copy-0").unwrap(),
                bounds
            );
            let focused = copy_pixels(cx, window, bounds);
            let accent = theme::theme().ui_accent();
            let expected = [(accent >> 16) as u8, (accent >> 8) as u8, accent as u8];
            assert!(
                focused
                    .0
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
                "Copy has an opaque accent focus-visible ring: {:?} versus {:?}",
                focused.0,
                expected
            );
            assert!(
                focused.1 < rest.1,
                "focused glyph is no longer opacity-dimmed"
            );
            press(cx, window, "shift-tab");
            cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
            paint(cx, window);
            paint(cx, window);
            let hovered = copy_pixels(cx, window, bounds);
            assert!(hovered.1 < rest.1, "hover restores glyph opacity");
            drop(panel);
            unmount(cx, app, window);
        }
    }
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
}
