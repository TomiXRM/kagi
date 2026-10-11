//! #1271 / #1272: native single-target selection and platform shortcut column.
use crate::macos::{build_fixture, repo_fingerprint, unmount};
use crate::surface_scrollbars::PresentationRestore;
use gpui::{point, px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::command_palette::{rows_for, PaletteAction, PaletteRow};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, keys: &str) {
    draw(cx, window);
    cx.simulate_keystrokes(window, keys);
    draw(cx, window);
}

fn mount(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().unwrap();
    (app, window.into())
}

fn query(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.command_palette_input
                .clone()
                .unwrap()
                .update(cx, |input, cx| {
                    input.set_value(text, window, cx);
                });
        });
    })
    .unwrap();
    draw(cx, window);
}

fn assert_selection(
    app: &Entity<KagiApp>,
    cx: &mut VisualTestAppContext,
    rows: &[PaletteRow],
    selected: usize,
) {
    assert_eq!(
        cx.read(|cx| app.read(cx).command_palette_selected),
        selected,
        "real pointer movement must move the activation target"
    );
    let ax = kagi::ui::list_a11y::recorded_list("command-palette-list").unwrap();
    assert_eq!(ax.role, Some(gpui::Role::ListBox));
    assert_eq!(ax.label, i18n::Msg::CommandPalettePlaceholder.t());
    assert_eq!(
        ax.rows.values().filter(|(_, selected)| *selected).count(),
        1
    );
    assert_eq!(
        ax.rows.get(&selected),
        Some(&(rows[selected].label.clone(), true))
    );
}

fn assert_single_fill(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    scroll: &gpui::ScrollHandle,
    selected: usize,
) {
    cx.update_window(window, |_, window, _cx| {
        let image = window.render_to_image().expect("native palette fill");
        let scale = window.scale_factor();
        let mut highlighted = Vec::new();
        for index in 0..scroll.children_count() {
            let bounds = scroll.bounds_for_item(index).unwrap();
            let y = bounds.center().y + scroll.offset().y;
            if y <= scroll.bounds().top() || y >= scroll.bounds().bottom() {
                continue;
            }
            let color = image
                .get_pixel(
                    ((bounds.left() + px(2.)) * scale).into(),
                    (y * scale).into(),
                )
                .0;
            let expected = theme::theme().selected;
            let rgb = [
                (expected >> 16) as u8,
                (expected >> 8) as u8,
                expected as u8,
            ];
            if color[..3]
                .iter()
                .zip(rgb)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 2)
            {
                highlighted.push(index);
            }
        }
        assert_eq!(
            highlighted,
            vec![selected],
            "only the Enter target may paint selection fill"
        );
    })
    .unwrap();
}

fn hover_target(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    rows: &[PaletteRow],
    index: usize,
) -> gpui::Point<gpui::Pixels> {
    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
    scroll.scroll_to_item(index);
    draw(cx, window);
    let bounds = scroll.bounds_for_item(index).unwrap();
    let at = bounds.center() + point(px(0.), scroll.offset().y);
    cx.simulate_mouse_move(window, at, None, Modifiers::none());
    draw(cx, window);
    assert_selection(app, cx, rows, index);
    assert_single_fill(cx, window, &scroll, index);
    at
}

fn highlight_case(cx: &mut VisualTestAppContext, repo: &std::path::Path, dimensions: (f32, f32)) {
    let (app, window) = mount(cx, repo, dimensions);
    keys(cx, window, "cmd-p down");
    let rows = cx.read(|cx| rows_for(app.read(cx), ""));
    let target = rows
        .iter()
        .position(|row| row.action == PaletteAction::Command("view.toggleSidebar"))
        .unwrap();
    let at = hover_target(cx, &app, window, &rows, target);
    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
    let offset = scroll.offset();
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-80.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, window);
    assert_ne!(
        scroll.offset(),
        offset,
        "real wheel must move content under resting pointer"
    );
    // GPUI can resend the pointer coordinates after layout/scroll; this is not motion.
    cx.simulate_mouse_move(window, at, None, Modifiers::none());
    draw(cx, window);
    assert_selection(&app, cx, &rows, target);
    let before = cx.read(|cx| app.read(cx).sidebar.visible);
    keys(cx, window, "enter");
    assert_eq!(
        cx.read(|cx| app.read(cx).sidebar.visible),
        !before,
        "Enter runs hovered/highlighted identity"
    );
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    keys(cx, window, "cmd-p");
    let disabled = rows.iter().position(|row| !row.enabled).unwrap();
    hover_target(cx, &app, window, &rows, disabled);
    keys(cx, window, "enter");
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "disabled highlighted row cannot execute"
    );
    drop(scroll);
    unmount(cx, app, window);
}

pub fn scenario_palette_highlight(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for preset in ["apple-light", "apple-dark"] {
                theme::set_active(preset);
                for locale in ["en", "ja"] {
                    std::env::set_var("KAGI_LANG", locale);
                    i18n::init_lang();
                    highlight_case(cx, fixture.path(), dimensions);
                }
            }
        }
    }
    assert_eq!(repo_fingerprint(fixture.path()), before);
    eprintln!("[gui-e2e] PASS palette_highlight");
}

fn shortcut_case(cx: &mut VisualTestAppContext, repo: &std::path::Path, dimensions: (f32, f32)) {
    let (app, window) = mount(cx, repo, dimensions);
    keys(cx, window, "cmd-p");
    let rows = cx.read(|cx| rows_for(app.read(cx), ""));
    assert_shortcut_column(window, &rows);
    keys(cx, window, "escape");
    let before = cx.read(|cx| app.read(cx).sidebar.visible);
    keys(cx, window, "ctrl-alt-shift-cmd-k");
    assert_eq!(
        cx.read(|cx| app.read(cx).sidebar.visible),
        !before,
        "remapped glyph and registered action must agree"
    );
    keys(cx, window, "cmd-p");
    query(
        cx,
        &app,
        window,
        kagi::ui::command_palette::command_label("repo.fetch", i18n::lang()),
    );
    let unbound = cx.read(|cx| {
        rows_for(
            app.read(cx),
            kagi::ui::command_palette::command_label("repo.fetch", i18n::lang()),
        )
    });
    assert!(unbound
        .iter()
        .filter(|row| row.action == PaletteAction::Command("repo.fetch"))
        .all(|row| row.keystroke.is_none()));
    unmount(cx, app, window);
}

fn assert_shortcut_column(window: AnyWindowHandle, rows: &[PaletteRow]) {
    let mut right = None;
    for (index, row) in rows.iter().enumerate() {
        let Some(_) = &row.keystroke else {
            continue;
        };
        let PaletteAction::Command(id) = &row.action else {
            panic!("only registry commands carry shortcuts");
        };
        let binding = kagi::ui::commands::effective_keystroke(id).unwrap();
        let expected = gpui_component::kbd::Kbd::format(&gpui::Keystroke::parse(&binding).unwrap());
        assert_eq!(
            e2e::palette::recorded_shortcut(index).unwrap().as_ref(),
            expected,
            "macOS rendered shortcut must use pinned Kbd glyphs, not Cmd+ text"
        );
        let shortcut =
            e2e::control_bounds(window.window_id(), &format!("palette-shortcut-{index}")).unwrap();
        let label =
            e2e::control_bounds(window.window_id(), &format!("palette-label-{index}")).unwrap();
        let card = e2e::control_bounds(window.window_id(), "palette-card").unwrap();
        assert!(shortcut.size.width > px(0.));
        assert!(
            label.right() < shortcut.left(),
            "label and shortcut column must not intersect"
        );
        assert!(
            shortcut.right() < card.right(),
            "shortcut fits inside trailing padding"
        );
        if let Some(right) = right {
            assert!(
                (shortcut.right() - right).abs() <= px(1.),
                "shortcuts share right-aligned column"
            );
        }
        right = Some(shortcut.right());
        if *id == "view.toggleSidebar" {
            assert_eq!(expected, "⌃⌥⇧⌘K");
        }
        if *id == "file.openRepository" {
            assert_eq!(expected, "⇧⌘O");
        }
        if *id == "view.toggleInspector" {
            assert_eq!(expected, "⌘⌫");
        }
    }
}
pub fn scenario_palette_shortcut_glyphs(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let fixture = build_fixture();
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for preset in ["apple-light", "apple-dark"] {
                theme::set_active(preset);
                for locale in ["en", "ja"] {
                    std::env::set_var("KAGI_LANG", locale);
                    i18n::init_lang();
                    shortcut_case(cx, fixture.path(), dimensions);
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS palette_shortcut_glyphs");
}

pub fn install_shortcut_bindings() -> crate::gui_isolation::SavedKeys {
    let _bindings = crate::gui_isolation::SavedKeys::keep(&[
        "keybinding.view.toggleSidebar",
        "keybinding.view.toggleInspector",
        "keybinding.repo.fetch",
        "keybinding.file.openRepository",
    ]);
    settings::write_setting(
        "keybinding.view.toggleSidebar",
        Some("ctrl-alt-shift-secondary-k"),
    );
    settings::write_setting(
        "keybinding.view.toggleInspector",
        Some("secondary-backspace"),
    );
    settings::write_setting("keybinding.repo.fetch", Some(""));
    settings::write_setting("keybinding.file.openRepository", Some("secondary-shift-o"));
    _bindings
}
