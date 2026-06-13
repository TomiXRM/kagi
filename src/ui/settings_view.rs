//! T-SETTINGS-001 / ADR-0080: the OpenLogi-style Settings window.
//!
//! A thin view over the existing preference storage (`theme.rs`'s
//! `read_setting`/`write_setting`, the `theme::*` / `i18n::*` apply paths). It
//! never touches the repository (ADR-0078 invariant — the settings view makes
//! no git calls, so the `src/ui/` repo-access grep gate stays at 0).
//!
//! # Structure (mirrors `gpui_component::setting`)
//!
//! ```ignore
//! Settings::new("kagi-settings").sidebar_width(px(210.))
//!   SettingPage("Appearance")
//!     SettingGroup
//!       SettingItem("Theme",         SettingField::dropdown(..))   // 6 themes
//!       SettingItem("UI Zoom",       SettingField::number_input(..))
//!       SettingItem("Compact graph", SettingField::switch(..))
//!   SettingPage("Language")
//!     SettingGroup
//!       SettingItem("Interface language", SettingField::dropdown(..)) // EN/JA
//! ```
//!
//! # Live apply + persist
//!
//! Every field's `set_value` closure runs on the same `&mut App` the widget
//! renders on. Each one reuses the existing apply path (`KagiApp::set_theme`,
//! `theme::set_zoom`, `KagiApp::set_lang`, `theme::set_compact_graph`) — which
//! already persist to `settings.json` and `cx.notify()` — by routing through the
//! `KagiApp` entity, so the whole UI repaints immediately and the choice
//! survives a restart. No new persistence layer is introduced.

use gpui::{
    px, AnyElement, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, Styled as _, div,
};
use gpui_component::setting::{
    NumberFieldOptions, Settings, SettingField, SettingGroup, SettingItem, SettingPage,
};

use super::i18n::{self, Lang, Msg};
use super::theme::{self, theme};
use super::KagiApp;

/// Build the centred Settings overlay (panel over a click-to-dismiss scrim).
///
/// `app` is the live `KagiApp` entity; the field closures route their apply
/// through it so the existing `set_theme` / `set_lang` paths (terminal config,
/// menu-bar `✓` marker, `cx.notify()`) run unchanged.
pub fn render_settings_overlay(app: Entity<KagiApp>, cx: &mut gpui::Context<KagiApp>) -> AnyElement {
    let settings = build_settings(app);

    // Dismiss when the scrim is clicked (mirrors `commands::wrap_overlay`).
    let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
        this.menu_overlay = None;
        cx.stop_propagation();
        cx.notify();
    });
    // Swallow clicks inside the panel so they don't bubble to the scrim.
    let eat = |s: gpui::Div| {
        s.on_mouse_down(MouseButton::Left, |_, _w, cx| cx.stop_propagation())
    };

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(rgb(theme().bg_base))
                .opacity(0.55)
                .on_mouse_down(MouseButton::Left, dismiss),
        )
        .child(
            eat(div())
                .w(px(820.0))
                .h(px(520.0))
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(10.0))
                .border_1()
                .border_color(rgb(theme().selected))
                .bg(rgb(theme().panel))
                .shadow_lg()
                .child(
                    // Title bar (domain-free prose → localized via Msg).
                    div()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(rgb(theme().selected))
                        .text_color(rgb(theme().color_branch))
                        .child(SharedString::from(Msg::SettingsTitle.t())),
                )
                .child(div().flex_1().min_h_0().child(settings)),
        )
        .into_any_element()
}

/// Assemble the `gpui_component::setting::Settings` element with its pages.
fn build_settings(app: Entity<KagiApp>) -> Settings {
    Settings::new("kagi-settings")
        .sidebar_width(px(210.0))
        .page(appearance_page(app.clone()))
        .page(language_page(app))
}

/// Appearance page: Theme · UI Zoom · Compact graph.
fn appearance_page(app: Entity<KagiApp>) -> SettingPage {
    // ── Theme dropdown: (slug, display-name) over the 6 built-ins ──
    let theme_options: Vec<(SharedString, SharedString)> = theme::THEMES
        .iter()
        .map(|t| (SharedString::from(t.slug), SharedString::from(t.name)))
        .collect();
    let theme_app = app.clone();
    let theme_field = SettingField::dropdown(
        theme_options,
        // get: the active theme slug.
        |_cx| SharedString::from(theme::theme().slug),
        // set: reuse `KagiApp::set_theme` (persists + syncs gpui-component +
        // rebuilds the menu ✓ marker + notify).
        move |slug, cx| {
            let slug = slug.to_string();
            theme_app.update(cx, |app, cx| app.set_theme(&slug, cx));
        },
    );

    // ── UI Zoom number input (0.7–1.5, 0.1 step) ──
    let zoom_app = app.clone();
    let zoom_field = SettingField::number_input(
        NumberFieldOptions {
            min: theme::ZOOM_MIN as f64,
            max: theme::ZOOM_MAX as f64,
            step: theme::ZOOM_STEP as f64,
        },
        |_cx| theme::zoom() as f64,
        // `theme::set_zoom` clamps + persists; notify the entity to repaint.
        move |z, cx| {
            theme::set_zoom(z as f32);
            zoom_app.update(cx, |_app, cx| cx.notify());
        },
    );

    // ── Compact graph switch (bound to KagiApp.graph_compact + persisted) ──
    let compact_app = app.clone();
    let compact_field = SettingField::switch(
        |_cx| theme::compact_graph(),
        move |on, cx| {
            compact_app.update(cx, |app, cx| {
                app.graph_compact = on;
                theme::set_compact_graph(on);
                cx.notify();
            });
        },
    );

    SettingPage::new(SharedString::from(Msg::SettingsAppearance.t()))
        .default_open(true)
        .group(
            SettingGroup::new()
                .item(
                    SettingItem::new(SharedString::from(Msg::SettingsTheme.t()), theme_field)
                        .description(Msg::SettingsThemeDesc.t()),
                )
                .item(
                    SettingItem::new(SharedString::from(Msg::SettingsZoom.t()), zoom_field)
                        .description(Msg::SettingsZoomDesc.t()),
                )
                .item(
                    SettingItem::new(SharedString::from(Msg::SettingsCompact.t()), compact_field)
                        .description(Msg::SettingsCompactDesc.t()),
                ),
        )
}

/// Language page: Interface language (English / 日本語).
fn language_page(app: Entity<KagiApp>) -> SettingPage {
    let lang_options: Vec<(SharedString, SharedString)> = vec![
        (SharedString::from(Lang::En.slug()), SharedString::from("English")),
        (SharedString::from(Lang::Ja.slug()), SharedString::from("日本語")),
    ];
    let lang_app = app;
    let lang_field = SettingField::dropdown(
        lang_options,
        |_cx| SharedString::from(i18n::lang().slug()),
        // Reuse `KagiApp::set_lang` (persists + rebuilds menu + notify).
        move |slug, cx| {
            if let Some(l) = Lang::from_slug(&slug) {
                lang_app.update(cx, |app, cx| app.set_lang(l, cx));
            }
        },
    );

    SettingPage::new(SharedString::from(Msg::SettingsLanguage.t())).group(
        SettingGroup::new().item(
            SettingItem::new(SharedString::from(Msg::SettingsInterfaceLang.t()), lang_field)
                .description(Msg::SettingsInterfaceLangDesc.t()),
        ),
    )
}

// Re-export `rgb` locally so the scrim/panel colour calls read the same way as
// the rest of `src/ui/`.
use gpui::rgb;
