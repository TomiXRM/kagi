//! Settings → Appearance controls for custom-theme files (#922).

use gpui::{
    div, rgb, AnyElement, Entity, IntoElement, ParentElement as _, SharedString, Styled as _,
};
use gpui_component::button::Button;
use gpui_component::text::TextView;
use gpui_component::{Disableable as _, Sizable as _};

use super::i18n::Msg;
use super::theme::{self, theme};
use super::{e2e, KagiApp, MONO_FONT};

/// Keep this group full-width: long paths and the Japanese button labels wrap
/// independently of the narrow label/control columns used by simple settings.
pub(super) fn section(app: &Entity<KagiApp>) -> impl IntoElement {
    let dir = theme::themes_dir();
    let display = dir
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| Msg::SettingsThemesFolderUnavailable.t().to_string());
    let app_open = app.clone();
    let app_reload = app.clone();
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .py_2()
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::SettingsCustomThemes.t())),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(Msg::SettingsCustomThemesDesc.t())),
        )
        .child(e2e::measure_control(
            "settings-custom-themes-path",
            div()
                .min_w_0()
                .w_full()
                .font_family(MONO_FONT)
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(
                    TextView::html(
                        "settings-custom-themes-path-text",
                        SharedString::from(escape_html(&display)),
                    )
                    .selectable(true),
                ),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(e2e::measure_control(
                    "settings-custom-themes-open",
                    Button::new("settings-custom-themes-open-button")
                        .label(Msg::SettingsCustomThemesOpen.t())
                        .outline()
                        .small()
                        .disabled(dir.is_none())
                        .on_click(move |_, _, cx| {
                            app_open.update(cx, |app, cx| app.open_themes_folder(cx));
                        }),
                ))
                .child(e2e::measure_control(
                    "settings-custom-themes-reload",
                    Button::new("settings-custom-themes-reload-button")
                        .label(Msg::SettingsCustomThemesReload.t())
                        .outline()
                        .small()
                        .on_click(move |_, window, cx| {
                            app_reload.update(cx, |app, cx| app.reload_themes(window, cx));
                        }),
                ))
                .child(guide_link()),
        )
}

/// TextView::html supports mouse selection; escape file names before parsing.
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// The custom-theme file format, every token and where it shows (#922).
const THEMES_GUIDE_URL: &str = "https://github.com/TomiXRM/kagi/blob/main/docs/themes.md";

fn guide_link() -> AnyElement {
    Button::new("settings-custom-themes-guide")
        .label(Msg::SettingsCustomThemesGuide.t())
        .outline()
        .small()
        .on_click(|_, _w, cx| cx.open_url(THEMES_GUIDE_URL))
        .into_any_element()
}
