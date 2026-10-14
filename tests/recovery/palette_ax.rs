//! #1513: actual AccessKit actions and native AppKit Press delivery.
use crate::macos::{build_fixture, mount, unmount};
use crate::native_ax;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    command_palette::{rows_for, PaletteAction},
    e2e, KagiApp,
};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, query: &str) {
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |app, cx| {
            app.open_command_palette(w, cx);
            app.command_palette_input
                .clone()
                .unwrap()
                .update(cx, |input, cx| input.set_value(query, w, cx));
        })
    })
    .unwrap();
    draw(cx, win);
}

pub fn scenario_palette_rows_ax_action(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    for locale in ["en", "ja"] {
        for preset in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                std::env::set_var("KAGI_LANG", locale);
                kagi_ui_core::i18n::init_lang();
                kagi_ui_core::theme::set_active(preset);
                kagi_ui_core::theme::set_zoom(zoom);
                let (app, win) = mount(cx, fixture.path());
                native_ax::activate(cx);
                for kind in ["command", "theme", "language"] {
                    let query = match kind {
                        "theme" => kagi_ui_core::i18n::Msg::SettingsTheme.t(),
                        "language" => kagi_ui_core::i18n::Msg::SettingsLanguage.t(),
                        _ => "",
                    };
                    open(cx, &app, win, query);
                    let rows = cx.read(|cx| rows_for(app.read(cx), query));
                    for (index, row) in rows.iter().enumerate() {
                        let node = native_ax::node(cx, win, ("palette-row", index).into());
                        assert_eq!(node["aria"]["role"], "ListBoxOption");
                        assert_eq!(node["aria"]["label"], row.label);
                        assert!(
                            node["aria"]["on_action"]
                                .as_array()
                                .is_some_and(|actions| actions.iter().any(|a| a == "Click")),
                            "palette row must expose native Click: {node}"
                        );
                    }
                    let target = rows
                        .iter()
                        .position(|row| {
                            row.enabled
                                && match (&row.action, kind) {
                                    (PaletteAction::SetTheme(slug), "theme") => {
                                        slug.as_ref() == preset
                                    }
                                    (PaletteAction::SetLang(lang), "language") => {
                                        *lang == kagi_ui_core::i18n::lang()
                                    }
                                    (PaletteAction::Command(id), "command") => {
                                        *id == "view.toggleSidebar"
                                    }
                                    _ => false,
                                }
                        })
                        .expect("exact enabled fixture action");
                    let action = rows[target].action.clone();
                    assert!(
                        native_ax::press(cx, win, &rows[target].label),
                        "native row Press is delivered"
                    );
                    draw(cx, win);
                    assert_eq!(e2e::palette::take_executed(), vec![action.clone()]);
                    open(cx, &app, win, query);
                    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                    scroll.scroll_to_item(target);
                    draw(cx, win);
                    let at = scroll.bounds_for_item(target).unwrap().center()
                        + gpui::point(gpui::px(0.), scroll.offset().y);
                    cx.simulate_click(win, at, gpui::Modifiers::none());
                    draw(cx, win);
                    assert_eq!(
                        e2e::palette::take_executed(),
                        vec![action.clone()],
                        "down/up must dispatch exactly once"
                    );
                    open(cx, &app, win, query);
                    for _ in 0..target {
                        cx.simulate_keystrokes(win, "down");
                    }
                    draw(cx, win);
                    assert_eq!(cx.read(|cx| app.read(cx).command_palette_selected), target);
                    cx.simulate_keystrokes(win, if target == 0 { "down up" } else { "up down" });
                    draw(cx, win);
                    assert_eq!(cx.read(|cx| app.read(cx).command_palette_selected), target);
                    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                    let selected = scroll.bounds_for_item(target).unwrap();
                    assert!(selected.top() + scroll.offset().y >= scroll.bounds().top());
                    assert!(selected.bottom() + scroll.offset().y <= scroll.bounds().bottom());
                    let node = native_ax::node(cx, win, ("palette-row", target).into());
                    assert_eq!(node["aria"]["selected"], true);
                    cx.simulate_keystrokes(win, "enter");
                    draw(cx, win);
                    assert_eq!(
                        e2e::palette::take_executed(),
                        vec![action],
                        "Input Enter dispatches highlighted identity"
                    );
                }
                open(cx, &app, win, "");
                let rows = cx.read(|cx| rows_for(app.read(cx), ""));
                let alternate_lang = rows.iter().find(|row| matches!(row.action, PaletteAction::SetLang(lang) if lang != kagi_ui_core::i18n::lang())).unwrap();
                assert!(native_ax::press(cx, win, &alternate_lang.label));
                draw(cx, win);
                assert_eq!(
                    e2e::palette::take_executed(),
                    vec![alternate_lang.action.clone()]
                );
                if let PaletteAction::SetLang(lang) = alternate_lang.action {
                    assert_eq!(kagi_ui_core::i18n::lang(), lang);
                }
                open(cx, &app, win, "");
                let rows = cx.read(|cx| rows_for(app.read(cx), ""));
                let disabled_index = rows
                    .iter()
                    .position(|row| !row.enabled)
                    .expect("disabled fixture command");
                let disabled = &rows[disabled_index];
                assert!(native_ax::press(cx, win, &disabled.label));
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                assert_eq!(
                    cx.read(|cx| app.read(cx).command_palette_selected),
                    disabled_index
                );
                let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                scroll.scroll_to_item(disabled_index);
                draw(cx, win);
                let at = scroll.bounds_for_item(disabled_index).unwrap().center()
                    + gpui::point(gpui::px(0.), scroll.offset().y);
                cx.simulate_click(win, at, gpui::Modifiers::none());
                cx.simulate_keystrokes(win, "enter");
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                open(cx, &app, win, "palette-no-matching-result-1513");
                assert!(cx
                    .read(|cx| rows_for(app.read(cx), "palette-no-matching-result-1513"))
                    .is_empty());
                cx.simulate_keystrokes(win, "up down enter");
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                unmount(cx, app, win);
            }
        }
    }
    eprintln!("[gui-e2e] PASS palette_rows_ax_action");
}
