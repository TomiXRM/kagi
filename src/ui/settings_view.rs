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

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::radio::RadioGroup;
use gpui_component::select::{Select, SelectItem, SelectState};
use gpui_component::switch::Switch;
use gpui_component::{IndexPath, Sizable as _};

use super::i18n::{self, Lang, Msg};
use super::theme::{self, theme};
use super::KagiApp;

/// The gpui-component `Select` state entity for the appearance theme picker.
/// Held by [`KagiApp`] (built in the window context, see `ui::run`).
pub type ThemeSelectState = SelectState<Vec<ThemeOption>>;

/// One theme entry shown in the appearance-section `Select`. `Value` is the
/// stable slug, which the `SelectEvent::Confirm` subscription feeds to
/// `KagiApp::set_theme`.
#[derive(Clone)]
pub struct ThemeOption {
    pub slug: &'static str,
    pub name: &'static str,
}

impl SelectItem for ThemeOption {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        SharedString::from(self.name)
    }

    fn value(&self) -> &Self::Value {
        &self.slug
    }
}

/// All registered themes as `Select` options.
pub fn theme_options() -> Vec<ThemeOption> {
    theme::THEMES
        .iter()
        .map(|t| ThemeOption {
            slug: t.slug,
            name: t.name,
        })
        .collect()
}

/// `IndexPath` of the active theme within [`theme_options`] (defaults to row 0).
pub fn current_theme_index() -> IndexPath {
    let cur = theme().slug;
    let row = theme::THEMES
        .iter()
        .position(|t| t.slug == cur)
        .unwrap_or(0);
    IndexPath::new(row)
}

/// Build the centred Settings overlay (panel over a click-to-dismiss scrim).
pub fn render_settings_overlay(
    app: Entity<KagiApp>,
    // The appearance-section theme picker's SelectState entity, passed in by the
    // caller (which holds `&self`) — we must NOT `app.read(cx)` here because this
    // renders *during* KagiApp's own update, which would panic ("cannot read …
    // while it is already being updated").
    theme_select: Option<Entity<ThemeSelectState>>,
    // Smart Commit state (detected models + current selection), passed in for the
    // same reason — never `app.read(cx)` during this render.
    smart: super::smart_commit::SmartCommitState,
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
                    Button::new("settings-close")
                        .label("✕")
                        .ghost()
                        .small()
                        .on_click(close_click),
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
                .child(appearance_section(&app, theme_select))
                .child(language_section(&app))
                .child(smart_commit_section(&app, &smart)),
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
                .child(div().text_color(rgb(theme().text_main)).child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .child(description),
                ),
        )
        .child(div().flex_shrink_0().child(control))
}

// ────────────────────────────────────────────────────────────
// Appearance
// ────────────────────────────────────────────────────────────

fn appearance_section(
    app: &Entity<KagiApp>,
    theme_select: Option<Entity<ThemeSelectState>>,
) -> impl IntoElement {
    // ── Theme picker (gpui-component Select) ──
    // The SelectState entity is built in the window context and held on KagiApp;
    // a `SelectEvent::Confirm` subscription applies + persists via set_theme.
    // Colours come from the Kagi → gpui-component theme bridge (sync_gpui_
    // component_theme). When the entity is absent (headless, pre-window) fall
    // back to a static label so the row still renders.
    let theme_dropdown = match theme_select {
        Some(state) => Select::new(&state)
            .menu_width(px(220.0))
            .small()
            .into_any_element(),
        None => {
            let cur = theme().slug;
            let cur_name = theme::THEMES
                .iter()
                .find(|t| t.slug == cur)
                .map(|t| t.name)
                .unwrap_or(cur);
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(cur_name))
                .into_any_element()
        }
    };

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
    let toggle = move |checked: &bool, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let on = *checked;
        app_c.update(cx, |app, cx| {
            app.graph_compact = on;
            theme::set_compact_graph(on);
            cx.notify();
        });
    };
    let compact_ctl = Switch::new("compact-toggle")
        .checked(compact)
        .on_click(toggle)
        .into_any_element();

    // ── Auto-fetch toggle ──
    let auto_fetch = theme::auto_fetch();
    let app_f = app.clone();
    let toggle_fetch = move |checked: &bool, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let on = *checked;
        app_f.update(cx, |_app, cx| {
            theme::set_auto_fetch(on);
            cx.notify();
        });
    };
    let auto_fetch_ctl = Switch::new("auto-fetch-toggle")
        .checked(auto_fetch)
        .on_click(toggle_fetch)
        .into_any_element();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(SharedString::from(
            Msg::SettingsAppearance.t(),
        )))
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
        .child(setting_row(
            SharedString::from(Msg::SettingsAutoFetch.t()),
            SharedString::from(Msg::SettingsAutoFetchDesc.t()),
            auto_fetch_ctl,
        ))
}

fn stepper_btn(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .label(label)
        .outline()
        .small()
        .on_click(on_click)
}

// ────────────────────────────────────────────────────────────
// Language
// ────────────────────────────────────────────────────────────

fn language_section(app: &Entity<KagiApp>) -> impl IntoElement {
    // Two-way segmented choice → gpui-component RadioGroup (stateless). The
    // on_click index maps back to the Lang ordering below.
    const LANGS: [(Lang, &str); 2] = [(Lang::En, "English"), (Lang::Ja, "日本語")];
    let cur = i18n::lang();
    let selected = LANGS.iter().position(|(l, _)| *l == cur);
    let app2 = app.clone();
    let chips = RadioGroup::horizontal("settings-language")
        .children(LANGS.map(|(_, label)| SharedString::from(label)))
        .selected_index(selected)
        .on_click(move |index: &usize, _w: &mut gpui::Window, cx: &mut gpui::App| {
            if let Some((lang, _)) = LANGS.get(*index) {
                let lang = *lang;
                app2.update(cx, |app, cx| app.set_lang(lang, cx));
            }
        });

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(SharedString::from(
            Msg::SettingsLanguage.t(),
        )))
        .child(setting_row(
            SharedString::from(Msg::SettingsInterfaceLang.t()),
            SharedString::from(Msg::SettingsInterfaceLangDesc.t()),
            chips.into_any_element(),
        ))
}

// ────────────────────────────────────────────────────────────
// Smart Commit (ADR-0090): pick the local LLM model used for commit messages.
// ────────────────────────────────────────────────────────────

fn smart_commit_section(
    app: &Entity<KagiApp>,
    smart: &super::smart_commit::SmartCommitState,
) -> impl IntoElement {
    let current = smart.model.clone();
    let models = smart.detected_models.clone();

    let control: AnyElement = if models.is_empty() {
        let note = match &current {
            Some(m) => format!("{} — start Ollama to switch", m),
            None => "No local models detected — start Ollama".to_string(),
        };
        div()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(note))
            .into_any_element()
    } else {
        let mut chips = div().flex().flex_row().flex_wrap().gap_2().justify_end();
        for m in models {
            let selected = current.as_deref() == Some(m.as_str());
            let (bg, fg, border) = if selected {
                (theme().selected, theme().text_main, theme().color_branch)
            } else {
                (theme().bg_base, theme().text_sub, theme().selected)
            };
            let app2 = app.clone();
            let m_for_handler = m.clone();
            let handler = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
                let model = m_for_handler.clone();
                app2.update(cx, |app, cx| {
                    app.smart_commit.set_model(model);
                    cx.notify();
                });
            };
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("sc-model-{}", m)))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(border))
                    .bg(rgb(bg))
                    .text_sm()
                    .text_color(rgb(fg))
                    .hover(|s| {
                        s.bg(rgb(theme().selected))
                            .text_color(rgb(theme().text_main))
                            .cursor_pointer()
                    })
                    .on_click(handler)
                    .child(SharedString::from(m.clone())),
            );
        }
        chips.into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(SharedString::from("Smart Commit")))
        .child(setting_row(
            SharedString::from("LLM model"),
            SharedString::from("Local Ollama model used to generate commit messages."),
            control,
        ))
}
