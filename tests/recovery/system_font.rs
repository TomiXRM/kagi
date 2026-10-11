//! #1068: native UI resolution and unchanged dense geometry at both zooms.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, FontWeight, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{fonts, i18n, settings, theme};

const HEIGHTS: [&str; 8] = [
    "toolbar-row",
    "local",
    "sidebar-local-main",
    "graph-commit-row",
    "cp-files",
    "cp-body",
    "cp-smart-suggest",
    "cp-commit-action",
];

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

fn select_font(cx: &mut VisualTestAppContext, source: settings::UiFontSource) {
    settings::set_ui_font_source(source);
    cx.update(|cx| {
        theme::sync_gpui_component_theme(cx);
        assert_eq!(
            gpui_component::Theme::global(cx).font_family.as_ref(),
            fonts::ui_font_family()
        );
        for weight in [FontWeight::NORMAL, FontWeight::SEMIBOLD, FontWeight::BOLD] {
            let mut font = theme::ui_font();
            font.weight = weight;
            let id = cx.text_system().resolve_font(&font);
            assert_eq!(
                cx.text_system()
                    .get_font_for_id(id)
                    .expect("resolved request")
                    .family,
                font.family,
                "requested family must resolve, not silently use the generic fallback"
            );
        }
    });
}

fn heights(cx: &VisualTestAppContext, window: AnyWindowHandle) -> [f32; 8] {
    HEIGHTS.map(|name| {
        let bounds = e2e::control_bounds(window.window_id(), name)
            .unwrap_or_else(|| panic!("missing rendered {name}"));
        assert!(bounds.size.height > gpui::px(0.), "{name}: {bounds:?}");
        let paint = e2e::control_paint(window.window_id(), name).expect("actual paint stage");
        cx.read(|cx| {
            let font = cx
                .text_system()
                .get_font_for_id(paint.font_id)
                .expect("native font");
            assert_eq!(
                font.family.as_ref(),
                fonts::ui_font_family(),
                "{name}: rendered inherited font must follow the preference"
            );
        });
        f32::from(bounds.size.height)
    })
}

fn compare(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    label: &str,
) {
    select_font(cx, settings::UiFontSource::Bundled);
    app.update(cx, |_, cx| cx.notify());
    draw(cx, window);
    let inter = heights(cx, window);
    select_font(cx, settings::UiFontSource::System);
    app.update(cx, |_, cx| cx.notify());
    draw(cx, window);
    let system = heights(cx, window);
    for (index, name) in HEIGHTS.into_iter().enumerate() {
        let delta = (inter[index] - system[index]).abs();
        eprintln!(
            "[gui-e2e] system_font metrics {label} {name}: Inter={} System={} delta={delta}",
            inter[index], system[index]
        );
        assert!(
            delta <= 1.,
            "{label}/{name}: font switch shifts height by {delta}px"
        );
    }
}

pub fn scenario_system_font(cx: &mut VisualTestAppContext) {
    let restore = crate::recovery_layout::GlobalSettings::capture();
    let saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "ui_zoom",
        "lang",
        "ui_font_source",
        "toolbar_display",
    ]);
    settings::write_setting("ui_font_source", None);
    settings::init_ui_font_source();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.set_theme("apple-light", cx));
    assert_eq!(
        theme::ui_font().family.as_ref(),
        ".AppleSystemUIFont",
        "Apple Light macOS must resolve the native system UI family"
    );
    for macos in [false, true] {
        for slug in ["apple-light", "apple-dark", "one-dark", "custom-apple"] {
            let expected = if macos && matches!(slug, "apple-light" | "apple-dark") {
                fonts::SYSTEM_UI_FONT
            } else {
                fonts::UI_FONT
            };
            assert_eq!(
                fonts::ui_font_family_for(settings::UiFontSource::System, slug, macos),
                expected
            );
        }
    }
    app.update(cx, |app, cx| app.set_theme("one-dark", cx));
    assert_eq!(theme::ui_font().family.as_ref(), "Inter");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    for slug in ["apple-light", "apple-dark"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for locale in [i18n::Lang::En, i18n::Lang::Ja] {
            app.update(cx, |app, cx| app.set_lang(locale, cx));
            for display in [
                settings::ToolbarDisplay::IconOnly,
                settings::ToolbarDisplay::IconAndText,
            ] {
                settings::set_toolbar_display(display);
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    compare(
                        cx,
                        &app,
                        window,
                        &format!("{slug}/{locale:?}/{display:?}@{zoom}"),
                    );
                }
            }
        }
    }
    unmount(cx, app, window);
    drop(restore);
    drop(saved);
    settings::init_ui_font_source();
    settings::init_toolbar_display();
    cx.update(theme::sync_gpui_component_theme);
    eprintln!("[gui-e2e] PASS system_font: native Apple font resolves at normal/semibold/bold; Linux/non-Apple retain Inter; toolbar/sidebar/graph/commit panel heights differ by <=1px at 100%/167% EN/JA Light/Dark in both toolbar modes");
}
