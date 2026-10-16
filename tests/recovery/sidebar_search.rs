//! Two-stage search cancellation and the native localized Clear action (#1547/#1548).
use crate::{
    keyboard_nav::keys,
    macos::{build_fixture, mount, unmount},
    native_ax,
    recovery_operations::paint,
};
use gpui::{Focusable, VisualTestAppContext};
use kagi_ui_core::{
    i18n::{Lang, Msg},
    theme,
};

fn matrix(
    cx: &mut VisualTestAppContext,
    desktop_search: bool,
    mut check: impl FnMut(
        &mut VisualTestAppContext,
        &gpui::Entity<kagi::ui::KagiApp>,
        gpui::AnyWindowHandle,
    ),
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let language = kagi_ui_core::i18n::lang();
    let zoom = theme::zoom();
    let fixture = build_fixture();
    let original_theme = theme::theme().slug.clone();
    for lang in [Lang::En, Lang::Ja] {
        for slug in ["apple-light", "apple-dark"] {
            for scale in [1., 1.67] {
                for dimensions in [(1400_f32, 900.), (1000., 720.)] {
                    theme::set_zoom(scale);
                    // Two-stage Escape belongs to inline search; compact Escape
                    // dismisses the navigator. AX Clear keeps the general matrix.
                    let dimensions = if desktop_search {
                        (
                            dimensions.0.max(
                                (theme::rem_size_px() * theme::compact_threshold_rem() as f32)
                                    .ceil(),
                            ),
                            dimensions.1,
                        )
                    } else {
                        dimensions
                    };
                    let (app, window) = mount(cx, fixture.path());
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                    });
                    crate::pr_detail_adaptive::resize(cx, window, dimensions);
                    paint(cx, window);
                    check(cx, &app, window);
                    unmount(cx, app, window);
                }
            }
        }
    }
    kagi_ui_core::i18n::set_lang(language);
    theme::set_zoom(zoom);
    theme::set_active(&original_theme);
}

pub fn sidebar_search_escape(cx: &mut VisualTestAppContext) {
    matrix(cx, true, |cx, app, window| {
        keys(cx, window, "cmd-f f e a t u r e");
        paint(cx, window);
        let input = cx.read(|cx| app.read(cx).sidebar.filter.clone().unwrap());
        keys(cx, window, "escape");
        paint(cx, window);
        assert!(cx.read(|cx| input.read(cx).value().is_empty()));
        cx.update_window(window, |_, w, cx| {
            assert!(
                input.read(cx).focus_handle(cx).is_focused(w),
                "first Escape clears without leaving search"
            )
        })
        .unwrap();
        keys(cx, window, "n e x t");
        assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), "next");
        keys(cx, window, "escape escape");
        paint(cx, window);
        cx.update_window(window, |_, w, cx| {
            assert!(
                app.read(cx).sidebar_row_focused_for_e2e(w).is_some(),
                "second Escape returns to list"
            )
        })
        .unwrap();
        keys(cx, window, "cmd-f escape");
        paint(cx, window);
        cx.update_window(window, |_, w, cx| {
            assert!(
                app.read(cx).sidebar_row_focused_for_e2e(w).is_some(),
                "empty Escape exits immediately"
            )
        })
        .unwrap();
        keys(cx, window, "cmd-f");
        cx.update_window(window, |_, w, cx| {
            input.update(cx, |input, cx| {
                use gpui::EntityInputHandler;
                input.replace_and_mark_text_in_range(None, "変換", None, w, cx);
            });
        })
        .unwrap();
        keys(cx, window, "escape");
        paint(cx, window);
        assert!(
            !cx.read(|cx| input.read(cx).value().is_empty()),
            "IME Escape must not clear search"
        );
        cx.update_window(window, |_, w, cx| {
            assert!(input.read(cx).focus_handle(cx).is_focused(w))
        })
        .unwrap();
    });
}

pub fn sidebar_search_clear_ax(cx: &mut VisualTestAppContext) {
    matrix(cx, false, |cx, app, window| {
        native_ax::activate(cx);
        keys(cx, window, "cmd-f f e a t u r e");
        paint(cx, window);
        let label = match kagi_ui_core::i18n::lang() {
            Lang::En => "Clear search",
            Lang::Ja => "検索内容を消去",
        };
        let tree = native_ax::tree(cx, window);
        let node = tree["nodes"]
            .as_object()
            .unwrap()
            .values()
            .find(|node| node["aria"]["label"] == label)
            .unwrap_or_else(|| panic!("native Clear button needs localized name {label}: {tree}"));
        assert_eq!(node["aria"]["role"], "Button");
        let input = cx.read(|cx| app.read(cx).sidebar.filter.clone().unwrap());
        let focused = native_ax::focused(cx, window);
        assert_eq!(focused["aria"]["role"], "TextInput");
        assert_eq!(focused["aria"]["label"], Msg::A11ySidebarFilter.t());
        assert!(
            native_ax::press(cx, window, label),
            "native AXPress clears search"
        );
        cx.run_until_parked();
        paint(cx, window);
        assert!(cx.read(|cx| input.read(cx).value().is_empty()));
        cx.update_window(window, |_, w, cx| {
            assert!(input.read(cx).focus_handle(cx).is_focused(w))
        })
        .unwrap();
        let tree = native_ax::tree(cx, window);
        assert!(
            !tree["nodes"]
                .as_object()
                .unwrap()
                .values()
                .any(|node| node["aria"]["label"] == label),
            "empty search removes Clear"
        );
        keys(cx, window, "p o i n t e r");
        paint(cx, window);
        let bounds = kagi::ui::e2e::control_bounds(window.window_id(), "sidebar-filter")
            .expect("rendered sidebar filter bounds");
        let point = gpui::point(bounds.right() - theme::scaled_px(20.), bounds.center().y);
        cx.simulate_click(window, point, gpui::Modifiers::none());
        cx.run_until_parked();
        paint(cx, window);
        assert!(cx.read(|cx| input.read(cx).value().is_empty()));
        let focused = native_ax::focused(cx, window);
        assert_eq!(focused["aria"]["role"], "TextInput");
        assert_eq!(focused["aria"]["label"], Msg::A11ySidebarFilter.t());
    });
}
