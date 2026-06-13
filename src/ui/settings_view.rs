//! T-SETTINGS-001 / ADR-0080: the OpenLogi-style Settings window.
//!
//! A thin view over the existing preference storage (`theme.rs`'s
//! `read_setting`/`write_setting` and the `theme::*` / `i18n::*` apply paths). It
//! never touches the repository (ADR-0078 — the settings view makes no git calls,
//! so the `src/ui/` repo-access grep gate stays at 0).
//!
//! # Rendering note
//!
//! This is built from **Kagi-native** elements coloured through `theme()` (the
//! single source of truth, ADR-0036) rather than `gpui_component::setting`: the
//! latter rendered setting titles/controls with the wrong (near-invisible)
//! foreground under Kagi's one-way theme bridge. Native chips/steppers give us
//! guaranteed contrast and full control of the layout.
//!
//! # Live apply + persist
//!
//! Every control's click handler reuses the existing apply path
//! (`KagiApp::set_theme`, `theme::set_zoom`, `KagiApp::set_lang`,
//! `theme::set_compact_graph`) — which already persist to `settings.json` and
//! `cx.notify()` — so the whole UI repaints immediately and the choice survives a
//! restart. No new persistence layer is introduced.

use gpui::{
    div, px, rgb, AnyElement, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};

use super::i18n::{self, Lang, Msg};
use super::theme::{self, theme};
use super::KagiApp;

/// Build the centred Settings overlay (panel over a click-to-dismiss scrim).
pub fn render_settings_overlay(
    app: Entity<KagiApp>,
    // Theme-dropdown expanded state. Passed in by the caller (which holds `&self`)
    // — we must NOT `app.read(cx)` here because this renders *during* KagiApp's own
    // update, which would panic ("cannot read … while it is already being updated").
    theme_open: bool,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
        this.menu_overlay = None;
        cx.stop_propagation();
        cx.notify();
    });
    let close_click = cx.listener(|this, _: &gpui::ClickEvent, _w, cx| {
        this.menu_overlay = None;
        cx.notify();
    });

    let panel = div()
        .w(px(640.0))
        .h(px(560.0))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(12.0))
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().panel))
        .shadow_lg()
        // swallow clicks inside the panel so they don't dismiss via the scrim
        .on_mouse_down(MouseButton::Left, |_, _w, cx| cx.stop_propagation())
        // ── Title bar ──────────────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .child(
                    div()
                        .text_color(rgb(theme().text_main))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(SharedString::from(Msg::SettingsTitle.t())),
                )
                .child(
                    div()
                        .id("settings-close")
                        .px_2()
                        .py_px()
                        .rounded_md()
                        .text_color(rgb(theme().text_sub))
                        .hover(|s| s.bg(rgb(theme().selected)).text_color(rgb(theme().text_main)).cursor_pointer())
                        .on_click(close_click)
                        .child(SharedString::from("✕")),
                ),
        )
        // ── Scrollable content: Appearance + Language sections ─────
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .px_5()
                .py_4()
                .flex()
                .flex_col()
                .gap_6()
                .child(appearance_section(&app, theme_open))
                .child(language_section(&app)),
        );

    // Scrim + centred panel.
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
                .opacity(0.6)
                .on_mouse_down(MouseButton::Left, dismiss),
        )
        .child(panel)
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Section / row helpers
// ────────────────────────────────────────────────────────────

fn section_header(title: SharedString) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme().color_branch))
        .pb_1()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(title)
}

/// One setting row: label + description (left) and a control (right).
fn setting_row(
    title: SharedString,
    description: SharedString,
    control: AnyElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_4()
        .py_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .child(
                    div()
                        .text_color(rgb(theme().text_main))
                        .child(title),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .child(description),
                ),
        )
        .child(div().flex_shrink_0().child(control))
}

/// A small clickable chip; `selected` highlights the current value.
fn chip(
    id: &'static str,
    label: SharedString,
    selected: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let (bg, fg, border) = if selected {
        (theme().selected, theme().text_main, theme().color_branch)
    } else {
        (theme().bg_base, theme().text_sub, theme().selected)
    };
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(bg))
        .text_sm()
        .text_color(rgb(fg))
        .hover(|s| s.bg(rgb(theme().selected)).text_color(rgb(theme().text_main)).cursor_pointer())
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Appearance
// ────────────────────────────────────────────────────────────

fn appearance_section(app: &Entity<KagiApp>, theme_open: bool) -> impl IntoElement {
    let cur_slug = theme().slug;

    // ── Theme dropdown (Kagi-native pull-down) ──
    // Clickable "field" (current theme name + ▾ chevron) with an inline option
    // list rendered directly below when `theme_open`. All colours from theme()
    // to guarantee contrast under the Kagi theme bridge (no gpui Select widget).
    let cur_name = theme::THEMES
        .iter()
        .find(|t| t.slug == cur_slug)
        .map(|t| t.name)
        .unwrap_or(cur_slug);

    let app_toggle = app.clone();
    let toggle_open = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        app_toggle.update(cx, |a, cx| {
            a.settings_theme_open = !a.settings_theme_open;
            cx.notify();
        });
    };

    let field = div()
        .id("theme-field")
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .w(px(220.0))
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().bg_base))
        .text_sm()
        .text_color(rgb(theme().text_main))
        .hover(|s| s.border_color(rgb(theme().color_branch)).cursor_pointer())
        .on_click(toggle_open)
        .child(SharedString::from(cur_name))
        .child(
            div()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from("\u{25be}")),
        );

    let mut theme_dropdown = div()
        .flex()
        .flex_col()
        .gap_1()
        .items_end()
        .child(field);

    if theme_open {
        let mut options = div()
            .flex()
            .flex_col()
            .w(px(220.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .overflow_hidden();
        for t in theme::THEMES.iter() {
            let slug = t.slug;
            let is_cur = t.slug == cur_slug;
            let app2 = app.clone();
            let handler = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
                app2.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    app.settings_theme_open = false;
                    cx.notify();
                });
            };
            let mut row = div()
                .id(t.slug)
                .px_3()
                .py_1()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                .on_click(handler)
                .child(SharedString::from(t.name));
            if is_cur {
                row = row.bg(rgb(theme().selected));
            }
            options = options.child(row);
        }
        theme_dropdown = theme_dropdown.child(options);
    }

    // ── UI Zoom stepper:  [−]  110%  [+] ──
    let zoom = theme::zoom();
    let app_minus = app.clone();
    let dec = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let z = (theme::zoom() - theme::ZOOM_STEP).max(theme::ZOOM_MIN);
        theme::set_zoom(z);
        app_minus.update(cx, |_a, cx| cx.notify());
    };
    let app_plus = app.clone();
    let inc = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let z = (theme::zoom() + theme::ZOOM_STEP).min(theme::ZOOM_MAX);
        theme::set_zoom(z);
        app_plus.update(cx, |_a, cx| cx.notify());
    };
    let zoom_ctl = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(stepper_btn("zoom-dec", "−", dec))
        .child(
            div()
                .min_w(px(56.0))
                .text_center()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(format!("{:.0}%", zoom * 100.0))),
        )
        .child(stepper_btn("zoom-inc", "+", inc))
        .into_any_element();

    // ── Compact graph toggle ──
    let compact = theme::compact_graph();
    let app_c = app.clone();
    let toggle = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let on = !theme::compact_graph();
        app_c.update(cx, |app, cx| {
            app.graph_compact = on;
            theme::set_compact_graph(on);
            cx.notify();
        });
    };
    let compact_ctl = chip(
        "compact-toggle",
        SharedString::from(if compact { "On" } else { "Off" }),
        compact,
        toggle,
    );

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(SharedString::from(Msg::SettingsAppearance.t())))
        .child(setting_row(
            SharedString::from(Msg::SettingsTheme.t()),
            SharedString::from(Msg::SettingsThemeDesc.t()),
            theme_dropdown.into_any_element(),
        ))
        .child(setting_row(
            SharedString::from(Msg::SettingsZoom.t()),
            SharedString::from(Msg::SettingsZoomDesc.t()),
            zoom_ctl,
        ))
        .child(setting_row(
            SharedString::from(Msg::SettingsCompact.t()),
            SharedString::from(Msg::SettingsCompactDesc.t()),
            compact_ctl,
        ))
}

fn stepper_btn(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().bg_base))
        .text_color(rgb(theme().text_main))
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .on_click(on_click)
        .child(SharedString::from(label))
}

// ────────────────────────────────────────────────────────────
// Language
// ────────────────────────────────────────────────────────────

fn language_section(app: &Entity<KagiApp>) -> impl IntoElement {
    let cur = i18n::lang();
    let mut chips = div().flex().flex_row().gap_2().justify_end();
    for (lang, label) in [(Lang::En, "English"), (Lang::Ja, "日本語")] {
        let app2 = app.clone();
        let handler = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
            app2.update(cx, |app, cx| app.set_lang(lang, cx));
        };
        let id = lang.slug();
        chips = chips.child(chip(id, SharedString::from(label), lang == cur, handler));
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(SharedString::from(Msg::SettingsLanguage.t())))
        .child(setting_row(
            SharedString::from(Msg::SettingsInterfaceLang.t()),
            SharedString::from(Msg::SettingsInterfaceLangDesc.t()),
            chips.into_any_element(),
        ))
}