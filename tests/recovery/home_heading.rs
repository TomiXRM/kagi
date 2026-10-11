//! #1197: Home's recent heading stays inside its sidebar, with an ellipsis
//! instead of a hard clip and the navigator's quiet section typography.
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{FontWeight, VisualTestAppContext};
use kagi::ui::{
    e2e, home,
    i18n::{self, Lang, Msg},
    theme,
};

pub fn scenario_home_heading(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang", "theme", "ui_zoom"]);
    let previous = (i18n::lang(), theme::theme().slug.to_string(), theme::zoom());
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for lang in [Lang::Ja, Lang::En] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                // Preserve production geometry, cover the requested 300px width,
                // then stress a narrower pane to force actual JA truncation.
                for width in [None, Some(300.), Some(220.)] {
                    home::set_sidebar_width_for_e2e(width);
                    app.update(cx, |_, cx| cx.notify());
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let sidebar =
                        e2e::control_bounds(window.window_id(), "home-sidebar-bounds").unwrap();
                    let heading =
                        e2e::control_paint(window.window_id(), "home-recent-heading").unwrap();
                    assert!(
                        heading.bounds.left() >= sidebar.left()
                            && heading.bounds.right() <= sidebar.right(),
                        "heading bounds must stay within the sidebar: {heading:?} / {sidebar:?}"
                    );
                    let natural_width = cx
                        .update_window(window, |_, window, _| {
                            let text = Msg::HomeRecent.t();
                            let mut style = window.text_style();
                            style.font_weight = heading.font_weight;
                            window
                                .text_system()
                                .shape_line(
                                    text.into(),
                                    heading.font_size,
                                    &[style.to_run(text.len())],
                                    None,
                                )
                                .width
                        })
                        .unwrap();
                    let available = heading.bounds.size.width - gpui::px(16. * 0.9 * zoom);
                    assert!(natural_width <= available || heading.ellipsis, "Home heading is hard-clipped at production/narrow width: natural={natural_width:?}, available={available:?}, {preset} {lang:?} {zoom} width={width:?}");
                    assert!(
                        heading.ellipsis,
                        "Home heading must use native text ellipsis, not overflow clipping"
                    );
                    assert_eq!(heading.font_weight, FontWeight::SEMIBOLD);
                    assert!((f32::from(heading.font_size) - 12. * 0.9 * zoom).abs() < 0.1);
                    assert_eq!(
                        heading.text_color,
                        gpui::rgb(theme::theme().text_sub).into()
                    );
                    eprintln!("[gui-e2e] home_heading {preset} {lang:?} zoom={zoom} width={width:?}: natural={natural_width:?}, available={available:?}, ellipsis={}", heading.ellipsis);
                    if lang == Lang::Ja && zoom == 1.67 && width == Some(220.) {
                        assert!(
                            natural_width > available,
                            "JA at 167% must exercise truncation, not merely configure it"
                        );
                    }
                }
            }
        }
    }
    home::set_sidebar_width_for_e2e(None);
    i18n::set_lang(previous.0);
    theme::set_active(&previous.1);
    theme::set_zoom(previous.2);
    unmount(cx, app, window);
}
