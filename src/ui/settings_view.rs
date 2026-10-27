//! T-SETTINGS-001 / ADR-0080: the OpenLogi-style Settings window.
//!
//! A thin view over the existing preference storage (`settings::`) and the
//! `theme::*` / `i18n::*` apply paths. It
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

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, AnyElement, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};
use kagi_ui_core::tooltip::HoverTooltip as _;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::radio::RadioGroup;
use gpui_component::select::{Select, SelectItem, SelectState};
use gpui_component::{Disableable as _, FocusTrapElement as _, IndexPath, Sizable as _};

use super::i18n::{self, Msg};
#[cfg(feature = "gui-e2e")]
pub use super::keyboard_nav::recorded_switch_help;
use super::theme::{self, theme};
#[cfg(feature = "gui-e2e")]
pub use super::theme_select::{appearance_ax_for_e2e, take_theme_option_ax_for_e2e};
use super::{settings, KagiApp};

/// Native scenarios opt into the macOS host; the portable suite keeps testing
/// the non-macOS overlay implementation on the native renderer.
#[cfg(all(target_os = "macos", feature = "gui-e2e"))]
#[derive(Default)]
pub struct SettingsHostForE2e {
    pub native: bool,
}
#[cfg(all(target_os = "macos", feature = "gui-e2e"))]
impl gpui::Global for SettingsHostForE2e {}

impl KagiApp {
    /// Lazily build the multiline `analyze_ignore` editor input, seeded from the
    /// on-disk `analyze_ignore` file. Called when the Settings overlay opens so
    /// the editor has live text to show (the input needs `&mut Window`, which is
    /// unavailable during `render`).
    pub fn ensure_analyze_ignore_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_controls.view.is_none() {
            let view = cx.new(|_| {
                kagi_ui_settings::SettingsView::new(kagi_ui_settings::SettingsSnapshot {
                    language: i18n::lang(),
                })
            });
            cx.subscribe(&view, |app, _, event, cx| match event {
                kagi_ui_settings::SettingsEvent::LanguageRequested(language) => {
                    app.set_lang(*language, cx);
                }
            })
            .detach();
            self.settings_controls.view = Some(view);
        }
        if self.analyze_ignore_input.is_some() {
            return;
        }
        let content = settings::read_analyze_ignore_text();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(8, 22)
        });
        self.settings_controls.saved_ignore = Some(content.clone());
        input.update(cx, |s, cx| s.set_value(content, window, cx));
        cx.subscribe(&input, |_, _, event, cx| {
            if matches!(event, gpui_component::input::InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.analyze_ignore_input = Some(input);
    }
}

/// The gpui-component `Select` state entity for the appearance theme picker.
/// Held by [`KagiApp`] (built in the window context, see `ui::run`).
pub type ThemeSelectState = SelectState<Vec<ThemeOption>>;

/// One theme entry shown in the appearance-section `Select`. `Value` is the
/// stable slug, which the `SelectEvent::Confirm` subscription feeds to
/// `KagiApp::set_theme`. Owned, so a custom theme's option outlives the theme
/// reload that replaced it (#922).
#[derive(Clone)]
pub struct ThemeOption {
    pub slug: SharedString,
    pub name: SharedString,
}

impl SelectItem for ThemeOption {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        // SelectState keeps its options across language changes. Resolve the
        // display name at render time so both the trigger and dropdown switch.
        theme::theme_by_slug(&self.slug)
            .map(|theme| {
                SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(
                    theme.display_name(),
                ))
            })
            .unwrap_or_else(|| self.name.clone())
    }

    fn render(&self, _: &mut Window, _: &mut gpui::App) -> impl IntoElement {
        let name = self.title();
        #[cfg(feature = "gui-e2e")]
        super::theme_select::record_ax(
            &format!("theme-option:{}", self.slug),
            gpui::Role::ListBoxOption,
            &name,
            &self.slug,
            None,
            false,
        );
        div()
            .id(self.slug.clone())
            .role(gpui::Role::ListBoxOption)
            .aria_label(name.clone())
            .aria_value(self.slug.clone())
            .child(name)
    }

    fn value(&self) -> &Self::Value {
        &self.slug
    }
}

/// All registered themes (built-in and custom) as `Select` options.
pub fn theme_options() -> Vec<ThemeOption> {
    theme::themes_by_name()
        .into_iter()
        .map(|t| ThemeOption {
            slug: SharedString::from(t.slug.clone()),
            name: SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(
                t.display_name(),
            )),
        })
        .collect()
}

/// `IndexPath` of the active theme within [`theme_options`] (defaults to row 0).
pub fn current_theme_index() -> IndexPath {
    let cur = theme().key();
    let row = theme::themes_by_name()
        .iter()
        .position(|t| t.key() == cur)
        .unwrap_or(0);
    IndexPath::new(row)
}

/// Build the centred Settings overlay (panel over a click-to-dismiss scrim).
// Each piece of state is passed in because this renders during KagiApp's own
// update and must not `app.read(cx)` (see the parameter comments).
#[allow(clippy::too_many_arguments)]
pub(super) fn render_settings_overlay(
    app: Entity<KagiApp>,
    // The appearance-section theme picker's SelectState entity, passed in by the
    // caller (which holds `&self`) — we must NOT `app.read(cx)` here because this
    // renders *during* KagiApp's own update, which would panic ("cannot read …
    // while it is already being updated").
    theme_select: Option<Entity<ThemeSelectState>>,
    // The switches' focus handles, for the same reason.
    switches: super::keyboard_nav::FocusSlots,
    // The panel's focus-trap handle (#974), for the same reason. `None` only
    // when Settings is drawn without having been opened (no trap then).
    trap: Option<gpui::FocusHandle>,
    // Smart Commit state (detected models + current selection), passed in for the
    // same reason — never `app.read(cx)` during this render.
    smart: super::smart_commit::SmartCommitState,
    // The Analyze-ignore editor's InputState entity, passed in by the caller for
    // the same reason (no `app.read(cx)` during render). Built lazily via
    // `KagiApp::ensure_analyze_ignore_input` when the overlay opens.
    analyze_ignore_input: Option<Entity<InputState>>,
    controls: &super::settings_controls::SettingsControls,
    analyze_dirty: bool,
    reset: Option<super::settings_analyze::AnalyzeResetAlert>,
    scroll: gpui::ScrollHandle,
    notification_lane: Option<AnyElement>,
    notifications: Option<AnyElement>,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, window, cx| {
        this.menu_overlay = None;
        this.clear_settings_smart_consent(window.window_handle().window_id());
        cx.stop_propagation();
        cx.notify();
    });
    let close_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
        this.menu_overlay = None;
        this.clear_settings_smart_consent(window.window_handle().window_id());
        cx.notify();
    });

    // Size the panel to a fraction of the window so it always fits (and so the
    // content stays reachable by scrolling when zoomed), capped at the preferred
    // dimensions on large windows.
    let viewport = window.viewport_size();
    let panel_w = (f32::from(viewport.width) * 0.85).min(820.0);
    let panel_h = (f32::from(viewport.height) * 0.85).min(720.0);

    let panel = div()
        .relative()
        .w(px(panel_w))
        .h(px(panel_h))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(12.0))
        .border_1()
        .border_color(rgb(theme().elevated_border(theme().surface)))
        .bg(rgb(theme().elevated_surface(theme().panel)))
        .shadow_lg()
        .child(super::e2e::measure_inside("settings-frame"))
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
                .border_color(rgb(theme().elevated_border(theme().surface)))
                .child(super::e2e::measure_inside("settings-titlebar"))
                .child(
                    div()
                        .text_color(rgb(theme().text_main))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(SharedString::from(Msg::SettingsTitle.t())),
                )
                .child(super::e2e::measure_control(
                    "settings-close",
                    Button::new("settings-close")
                        .label("✕")
                        .ghost()
                        .small()
                        .on_click(close_click),
                )),
        )
        .when_some(notification_lane, |panel, lane| {
            panel.child(div().flex().justify_end().flex_shrink_0().child(lane))
        })
        // One persistent scroll owner for the whole body; the kit owns thumb
        // drag/visibility state under the stable scrollbar element id.
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(super::render_helpers::with_system_vertical_scrollbar(
                    "settings-scrollbar",
                    &scroll,
                    div()
                        .id("settings-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&scroll)
                        .px_5()
                        .py_4()
                        .flex()
                        .flex_col()
                        .gap_6()
                        .child(appearance_section(
                            &app,
                            theme_select,
                            &switches,
                            &scroll,
                            cx,
                        ))
                        .child(language_section(controls.view.as_ref()))
                        .child(smart_commit_section(
                            &app,
                            &smart,
                            &switches,
                            controls,
                            trap.clone(),
                            &scroll,
                            cx,
                        ))
                        .child(analyze_ignore_section(
                            &app,
                            analyze_ignore_input,
                            analyze_dirty,
                            controls.ignore_error.as_deref(),
                        )),
                    cx,
                ))
                .children(notifications)
                // Keep the host's header close button and outer backdrop available.
                .when_some(controls.smart_consent.clone(), |el, alert| {
                    el.child(super::settings_controls::smart_consent_alert(
                        &app, alert, cx,
                    ))
                }),
        );
    controls.repair_focus(
        true,
        matches!(smart.provider, super::smart_commit::SmartProvider::Ollama)
            && !smart.detected_models.is_empty(),
        trap.as_ref(),
        window,
        cx,
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
                // Like `modal_overlay`'s backdrop: block every mouse event,
                // the scroll wheel included, from reaching the panes behind
                // Settings. Without it, scrolling Settings also scrolled the
                // commit list underneath.
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss),
        )
        .child(match trap {
            // gpui-component's Root keeps Tab / Shift+Tab inside a trap while
            // the focus is in it; the portable `open_settings` path puts it there.
            Some(trap) => panel.focus_trap("settings-trap", &trap).into_any_element(),
            None => panel.into_any_element(),
        })
        .when_some(reset, |el, reset| {
            el.child(super::settings_analyze::reset_alert(&app, reset, cx))
        })
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Section / row helpers
// ────────────────────────────────────────────────────────────

fn section_header(name: &'static str, title: SharedString) -> impl IntoElement {
    div()
        .text_size(super::theme::scaled_px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme().text_sub))
        .pb_1()
        .border_b_1()
        .border_color(rgb(theme().elevated_border(theme().surface)))
        .child(super::e2e::measure_inside(name))
        .child(title)
}

/// One passive grouped-form surface, never a second scroll owner.
fn settings_group(name: &'static str) -> gpui::Div {
    div()
        .relative()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .px_3()
        .rounded(px(8.))
        .bg(rgb(theme().bg_base))
        .child(super::e2e::measure_inside(name))
}

/// One setting row: label + description (left) and a control (right).
fn setting_row(
    first: bool,
    title: SharedString,
    description: SharedString,
    control: AnyElement,
) -> gpui::Div {
    #[cfg(feature = "gui-e2e")]
    let control = super::e2e::measure_control(format!("settings-control-{title}"), control);
    #[cfg(feature = "gui-e2e")]
    let description_name = format!("settings-description-{title}");
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_4()
        .py_2()
        .when(!first, |row| {
            row.border_t_1()
                .border_color(rgb(theme().elevated_border(theme().surface)))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                // `min_w_0` lets this column shrink below its content's intrinsic
                // width so the right-hand control stays inside the pane. Without
                // it, an unbreakable label/description (CJK text has no word
                // boundaries) keeps its full width and pushes the control off the
                // (overflow-hidden) panel edge, hiding it.
                .min_w_0()
                .flex_1()
                .child(div().text_color(rgb(theme().text_main)).child(title))
                .child(
                    div()
                        .when(cfg!(feature = "gui-e2e"), |description| {
                            #[cfg(feature = "gui-e2e")]
                            let description =
                                description.child(super::e2e::measure_inside(description_name));
                            description
                        })
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .child(description),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .w(gpui::relative(0.48))
                .min_w_0()
                .flex()
                .justify_end()
                .child(control),
        )
}

fn setting_row_with_help(
    first: bool,
    id: &'static str,
    title: Msg,
    summary: Msg,
    detail: Msg,
    control: AnyElement,
) -> impl IntoElement {
    let help = div()
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .child(detail.t());
    #[cfg(feature = "gui-e2e")]
    let help = help.child(super::e2e::measure_inside(format!("{id}-text")));
    div()
        .flex()
        .flex_col()
        .w_full()
        .pb_2()
        .child(setting_row(
            first,
            title.t().into(),
            summary.t().into(),
            control,
        ))
        .child(help)
        .child(super::e2e::measure_inside(id))
        .id(id)
        .role(gpui::Role::Group)
        .aria_label(title.t())
        .hover_tooltip_builder((id, 0usize), move |window, cx| {
            #[cfg(feature = "gui-e2e")]
            record_settings_help(id, detail.t());
            gpui_component::tooltip::Tooltip::new(detail.t()).build(window, cx)
        })
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static SETTINGS_HELP: std::cell::RefCell<std::collections::HashMap<&'static str, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(feature = "gui-e2e")]
pub(super) fn record_settings_help(id: &'static str, text: &str) {
    SETTINGS_HELP.with(|help| help.borrow_mut().insert(id, text.to_string()));
}

#[cfg(feature = "gui-e2e")]
pub fn take_settings_help_for_e2e(id: &str) -> Option<String> {
    SETTINGS_HELP.with(|help| help.borrow_mut().remove(id))
}

// ────────────────────────────────────────────────────────────
// Appearance
// ────────────────────────────────────────────────────────────

pub(super) fn appearance_section(
    app: &Entity<KagiApp>,
    theme_select: Option<Entity<ThemeSelectState>>,
    switches: &super::keyboard_nav::FocusSlots,
    scroll: &gpui::ScrollHandle,
    cx: &gpui::App,
) -> impl IntoElement {
    // ── Theme picker (gpui-component Select) ──
    // The SelectState entity is built in the window context and held on KagiApp;
    // a `SelectEvent::Confirm` subscription applies + persists via set_theme.
    // Colours come from the Kagi → gpui-component theme bridge (sync_gpui_
    // component_theme). When the entity is absent (headless, pre-window) fall
    // back to a static label so the row still renders.
    let theme_dropdown = match theme_select {
        Some(state) => {
            let expanded = state.read(cx).is_open();
            let value = kagi_domain::text_safety::sanitize_control_bytes(theme().display_name());
            #[cfg(feature = "gui-e2e")]
            super::theme_select::record_ax(
                "settings-theme-select",
                gpui::Role::ComboBox,
                Msg::SettingsTheme.t(),
                &value,
                Some(expanded),
                false,
            );
            super::e2e::measure_control(
                "settings-theme-select",
                Select::new(&state)
                    .role(gpui::Role::ComboBox)
                    .aria_label(Msg::SettingsTheme.t())
                    .aria_value(value)
                    .aria_expanded(expanded)
                    .menu_width(px(220.0))
                    .small()
                    .w(px(220.0)),
            )
        }
        None => div()
            .text_sm()
            .text_color(rgb(theme().text_main))
            .child(SharedString::from(theme().display_name().to_string()))
            .into_any_element(),
    };

    // ── UI Zoom stepper:  [−]  110%  [+] ──
    let zoom = theme::zoom();
    let zoom_label = format!("{:.0}%", zoom * 100.0);
    #[cfg(feature = "gui-e2e")]
    super::theme_select::record_ax(
        "zoom-value",
        gpui::Role::Status,
        &i18n::settings_zoom_value(&zoom_label),
        &zoom_label,
        None,
        false,
    );
    #[cfg(feature = "gui-e2e")]
    super::e2e::record_settings_zoom_label(&zoom_label);
    let app_minus = app.downgrade();
    let dec = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        theme::set_zoom(theme::step_zoom(theme::zoom(), false));
        theme::sync_gpui_component_theme(cx);
        cx.refresh_windows();
        let _ = app_minus.update(cx, |app, cx| {
            app.apply_terminal_config(cx);
            cx.notify();
        });
    };
    let app_plus = app.downgrade();
    let inc = move |_: &gpui::ClickEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        theme::set_zoom(theme::step_zoom(theme::zoom(), true));
        theme::sync_gpui_component_theme(cx);
        cx.refresh_windows();
        let _ = app_plus.update(cx, |app, cx| {
            app.apply_terminal_config(cx);
            cx.notify();
        });
    };
    let increase_name = i18n::settings_zoom_action(true, &zoom_label);
    let zoom_ctl = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(stepper_btn(
            "zoom-dec",
            "−",
            i18n::settings_zoom_action(false, &zoom_label),
            theme::step_zoom(zoom, false) == zoom,
            false,
            dec,
            |increase| theme::step_zoom(theme::zoom(), increase) == theme::zoom(),
            scroll.clone(),
        ))
        .child(
            div()
                .id("zoom-value")
                .role(gpui::Role::Status)
                .aria_label(i18n::settings_zoom_value(&zoom_label))
                .min_w(px(56.0))
                .text_center()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(zoom_label)),
        )
        .child(stepper_btn(
            "zoom-inc",
            "+",
            increase_name,
            theme::step_zoom(zoom, true) == zoom,
            true,
            inc,
            |increase| theme::step_zoom(theme::zoom(), increase) == theme::zoom(),
            scroll.clone(),
        ))
        .into_any_element();

    // The five switches reach the keyboard and assistive technology through
    // `keyboard_nav::switch` (#970), one Tab stop each, in drawing order.
    use super::keyboard_nav::switch;
    let title = |msg: Msg| SharedString::from(msg.t());

    // ── Compact graph toggle ──
    let app_c = app.downgrade();
    let compact_ctl = switch(
        "compact-toggle",
        switches.get(0, cx),
        scroll.clone(),
        title(Msg::SettingsCompact),
        title(Msg::SettingsCompactDesc),
        theme::compact_graph(),
        move |on, _w, cx| {
            let _ = app_c.update(cx, |app, cx| {
                app.graph_compact = on;
                theme::set_compact_graph(on);
                cx.notify();
            });
        },
    );

    // ── Swimlane-visuals toggle ──
    // Flips the swimlane visuals (avatar commit nodes, lane tint band, lane
    // padding). The lane *layout* is always `Stable` (ADR-0122) and does not
    // depend on this flag; `reload()` re-snapshots so rebuilt rows pick the
    // visual change up immediately.
    let app_lc = app.downgrade();
    let lane_compact_ctl = switch(
        "lane-compact-toggle",
        switches.get(1, cx),
        scroll.clone(),
        title(Msg::SettingsLaneCompact),
        title(Msg::SettingsLaneCompactDesc),
        theme::graph_lane_compact(),
        move |on, _w, cx| {
            let _ = app_lc.update(cx, |app, cx| {
                theme::set_graph_lane_compact(on);
                app.reload(cx);
                cx.notify();
            });
        },
    );

    // ── Auto-fetch toggle ──
    let app_f = app.downgrade();
    let auto_fetch_ctl = switch(
        "auto-fetch-toggle",
        switches.get(2, cx),
        scroll.clone(),
        title(Msg::SettingsAutoFetch),
        title(Msg::SettingsAutoFetchDesc),
        theme::auto_fetch(),
        move |on, _w, cx| {
            let _ = app_f.update(cx, |_app, cx| {
                theme::set_auto_fetch(on);
                cx.notify();
            });
        },
    );

    // ── Reduce-motion toggle (issue #354 / ADR-0173) ──
    let app_rm = app.downgrade();
    let reduce_motion_ctl = switch(
        "reduce-motion-toggle",
        switches.get(3, cx),
        scroll.clone(),
        title(Msg::SettingsReduceMotion),
        title(Msg::SettingsReduceMotionDesc),
        theme::app_reduce_motion(),
        move |on, _w, cx| {
            let _ = app_rm.update(cx, |_app, cx| {
                theme::set_reduce_motion(on);
                cx.notify();
            });
        },
    );

    // ── Terminal auto-lock toggle (#772 / ADR-0208, default off) ──
    let app_al = app.downgrade();
    let auto_lock_ctl = switch(
        "terminal-auto-lock-toggle",
        switches.get(4, cx),
        scroll.clone(),
        title(Msg::SettingsTerminalAutoLock),
        title(Msg::SettingsTerminalAutoLockDesc),
        crate::ui::settings::terminal_auto_lock(),
        move |on, _w, cx| {
            let _ = app_al.update(cx, |_app, cx| {
                crate::ui::settings::set_terminal_auto_lock(on);
                cx.notify();
            });
        },
    );

    // ── Graph Cmd+C copy target (ADR-0170) ──
    // Two-way segmented choice → hash (default) or the row's local branch.
    const COPY_TARGETS: [(&str, &str); 2] = [("hash", "Hash"), ("branch", "Branch")];
    let cur_target = settings::read_setting("graph_copy_target");
    let cur_target = cur_target.as_deref().unwrap_or("hash");
    let copy_selected = COPY_TARGETS.iter().position(|(k, _)| *k == cur_target);
    let app_gc = app.downgrade();
    let copy_target_ctl = RadioGroup::horizontal("settings-graph-copy")
        .children(COPY_TARGETS.map(|(_, label)| SharedString::from(label)))
        .selected_index(copy_selected)
        .on_click(
            move |index: &usize, _w: &mut gpui::Window, cx: &mut gpui::App| {
                if let Some((key, _)) = COPY_TARGETS.get(*index) {
                    settings::write_setting("graph_copy_target", Some(key));
                    let _ = app_gc.update(cx, |_a, cx| cx.notify());
                }
            },
        )
        .into_any_element();

    let app_toolbar = app.downgrade();
    let toolbar_ctl = RadioGroup::horizontal("settings-toolbar-display")
        .children(
            [Msg::ToolbarIconAndText, Msg::ToolbarIconOnly].map(|msg| SharedString::from(msg.t())),
        )
        .selected_index(Some(
            if settings::toolbar_display(&theme().slug) == settings::ToolbarDisplay::IconOnly {
                1
            } else {
                0
            },
        ))
        .on_click(move |index: &usize, _, cx| {
            settings::set_toolbar_display(if *index == 1 {
                settings::ToolbarDisplay::IconOnly
            } else {
                settings::ToolbarDisplay::IconAndText
            });
            super::commands::publish_menus(cx);
            let _ = app_toolbar.update(cx, |_, cx| cx.notify());
        })
        .into_any_element();

    let app_markdown = app.downgrade();
    let markdown_ctl = RadioGroup::horizontal("settings-markdown-default-view")
        .children(
            [Msg::MarkdownDefaultPreview, Msg::MarkdownDefaultSource]
                .map(|msg| SharedString::from(msg.t())),
        )
        .selected_index(Some(usize::from(
            settings::Settings::load().editor_markdown_default_view()
                == settings::MarkdownDefaultView::Source,
        )))
        .on_click(move |index: &usize, _, cx| {
            let mode = if *index == 1 {
                settings::MarkdownDefaultView::Source
            } else {
                settings::MarkdownDefaultView::Preview
            };
            settings::write_setting("editor_markdown_default_view", Some(mode.as_str()));
            cx.refresh_windows();
            let _ = app_markdown.update(cx, |_, cx| cx.notify());
        })
        .into_any_element();

    let threshold = theme::compact_threshold_rem();
    let threshold_value = format!("{threshold}rem");
    let threshold_conversion =
        i18n::settings_compact_threshold_value(threshold, threshold as f32 * theme::rem_size_px());
    #[cfg(feature = "gui-e2e")]
    super::theme_select::record_ax(
        "compact-threshold-value",
        gpui::Role::Status,
        &threshold_conversion,
        &threshold_value,
        None,
        false,
    );
    let threshold_buttons = div()
        .flex()
        .items_center()
        .gap_2()
        .children([false, true].map(|increase| {
            let owner = app.downgrade();
            stepper_btn(
                if increase {
                    "compact-threshold-inc"
                } else {
                    "compact-threshold-dec"
                },
                if increase { "+" } else { "−" },
                i18n::settings_compact_threshold_action(increase, threshold),
                if increase {
                    threshold == kagi_domain::compact::MAX_THRESHOLD_REM
                } else {
                    threshold == kagi_domain::compact::MIN_THRESHOLD_REM
                },
                increase,
                move |_, _, cx| {
                    let current = theme::compact_threshold_rem();
                    theme::set_compact_threshold_rem(if increase {
                        current + 1
                    } else {
                        current - 1
                    });
                    cx.refresh_windows();
                    let _ = owner.update(cx, |_, cx| cx.notify());
                },
                |increase| {
                    theme::compact_threshold_rem()
                        == if increase {
                            kagi_domain::compact::MAX_THRESHOLD_REM
                        } else {
                            kagi_domain::compact::MIN_THRESHOLD_REM
                        }
                },
                scroll.clone(),
            )
        }));
    let threshold_ctl = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("compact-threshold-value")
                        .role(gpui::Role::Status)
                        .aria_label(threshold_conversion.clone())
                        .child(threshold_value),
                )
                .child(threshold_buttons),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(threshold_conversion),
        )
        .into_any_element();

    let appearance = settings_group("settings-appearance-group")
        .child(setting_row(
            true,
            SharedString::from(Msg::SettingsTheme.t()),
            SharedString::from(Msg::SettingsThemeDesc.t()),
            theme_dropdown.into_any_element(),
        ))
        .when(
            cfg!(target_os = "macos") && matches!(&*theme().slug, "apple-light" | "apple-dark"),
            |section| {
                let owner = app.downgrade();
                section.child(setting_row(
                    false,
                    Msg::SettingsAccent.t().into(),
                    theme::detected_system_accent().detected_label().into(),
                    RadioGroup::horizontal("settings-accent")
                        .children([
                            SharedString::from(Msg::SettingsAccentSystem.t()),
                            SharedString::from(Msg::SettingsAccentTheme.t()),
                        ])
                        .selected_index(Some(usize::from(!theme::follows_system_accent())))
                        .on_click(move |index: &usize, _, cx| {
                            theme::set_follows_system_accent(*index == 0);
                            theme::sync_gpui_component_theme(cx);
                            cx.refresh_windows();
                            let _ = owner.update(cx, |_, cx| cx.notify());
                        })
                        .into_any_element(),
                ))
            },
        )
        .child(super::settings_theme_folder::section(app))
        .child(setting_row(
            false,
            SharedString::from(Msg::SettingsToolbarDisplay.t()),
            SharedString::from(Msg::SettingsToolbarDisplayDesc.t()),
            toolbar_ctl,
        ))
        .child(setting_row(
            false,
            SharedString::from(Msg::SettingsZoom.t()),
            SharedString::from(Msg::SettingsZoomDesc.t()),
            zoom_ctl,
        ))
        .child(setting_row(
            false,
            Msg::SettingsCompactThreshold.t().into(),
            Msg::SettingsCompactThresholdDesc.t().into(),
            threshold_ctl,
        ));
    let graph = settings_group("settings-graph-group")
        .child(setting_row(
            true,
            SharedString::from(Msg::SettingsCompact.t()),
            SharedString::from(Msg::SettingsCompactDesc.t()),
            compact_ctl,
        ))
        .child(setting_row_with_help(
            false,
            "settings-avatar-help",
            Msg::SettingsLaneCompact,
            Msg::SettingsLaneCompactSummary,
            Msg::SettingsLaneCompactDesc,
            lane_compact_ctl,
        ));
    let network = settings_group("settings-network-group").child(setting_row_with_help(
        true,
        "settings-fetch-help",
        Msg::SettingsAutoFetch,
        Msg::SettingsAutoFetchSummary,
        Msg::SettingsAutoFetchDesc,
        auto_fetch_ctl,
    ));
    let behaviour = settings_group("settings-behaviour-group")
        .child(setting_row_with_help(
            true,
            "settings-copy-help",
            Msg::SettingsGraphCopy,
            Msg::SettingsGraphCopySummary,
            Msg::SettingsGraphCopyDesc,
            copy_target_ctl,
        ))
        .child(setting_row(
            false,
            Msg::SettingsMarkdownDefaultView.t().into(),
            Msg::SettingsMarkdownDefaultViewDesc.t().into(),
            markdown_ctl,
        ))
        .child(setting_row_with_help(
            false,
            "settings-motion-help",
            Msg::SettingsReduceMotion,
            Msg::SettingsReduceMotionSummary,
            Msg::SettingsReduceMotionDesc,
            reduce_motion_ctl,
        ))
        .child(setting_row_with_help(
            false,
            "settings-terminal-lock-help",
            Msg::SettingsTerminalAutoLock,
            Msg::SettingsTerminalAutoLockSummary,
            Msg::SettingsTerminalAutoLockDesc,
            auto_lock_ctl,
        ));
    div()
        .flex()
        .flex_col()
        .gap_6()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_header(
                    "settings-appearance-heading",
                    Msg::SettingsAppearance.t().into(),
                ))
                .child(appearance),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_header(
                    "settings-graph-heading",
                    Msg::SettingsGraph.t().into(),
                ))
                .child(graph),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_header(
                    "settings-network-heading",
                    Msg::SettingsNetwork.t().into(),
                ))
                .child(network),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_header(
                    "settings-behaviour-heading",
                    Msg::SettingsBehaviour.t().into(),
                ))
                .child(behaviour),
        )
}

// Keep rendered button state separate from the live bound checked after activation.
#[allow(clippy::too_many_arguments)]
fn stepper_btn(
    id: &'static str,
    label: &'static str,
    name: String,
    disabled: bool,
    increase: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    at_limit: fn(bool) -> bool,
    scroll: gpui::ScrollHandle,
) -> impl IntoElement {
    #[cfg(feature = "gui-e2e")]
    super::theme_select::record_ax(id, gpui::Role::Button, &name, "", None, disabled);
    let button = super::button_style::named_disabled_button(
        Button::new(id)
            .child(label)
            .outline()
            .small()
            .disabled(disabled),
        name,
        disabled,
    );
    super::e2e::measure_control(
        id,
        StepperButton {
            id,
            button,
            increase,
            on_click,
            at_limit,
            scroll,
        },
    )
}

/// Reuse the kit Button's keyed focus owner, without adding a second stop.
#[derive(IntoElement)]
struct StepperButton<F: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static> {
    id: &'static str,
    button: Button,
    increase: bool,
    on_click: F,
    at_limit: fn(bool) -> bool,
    scroll: gpui::ScrollHandle,
}

impl<F: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static> gpui::RenderOnce
    for StepperButton<F>
{
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let Self {
            id,
            button,
            increase,
            on_click,
            at_limit,
            scroll,
        } = self;
        // Button::render uses this same public keyed-state API under the same
        // element namespace. Capture its existing owner before installing the
        // listener; no extra handle/entity is allocated.
        let focus = window
            .use_keyed_state(gpui::ElementId::from(id), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let button = super::button_style::reveal_on_focus(
            button,
            id.into(),
            focus.clone(),
            scroll,
            window,
            cx,
        );
        let button = button.on_click(move |event, window, cx| {
            on_click(event, window, cx);
            if at_limit(increase) && focus.is_focused(window) {
                // Both stepper stops still exist in this frame. Hand focus to
                // the opposite direction before the clicked Button retires.
                if increase {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
        });
        gpui::RenderOnce::render(button, window, cx)
    }
}

// ────────────────────────────────────────────────────────────
// Language
// ────────────────────────────────────────────────────────────

pub(super) fn language_section(
    view: Option<&Entity<kagi_ui_settings::SettingsView>>,
) -> impl IntoElement {
    div().w_full().children(view.cloned())
}

// ────────────────────────────────────────────────────────────
// Smart Commit (ADR-0090): pick the local LLM model used for commit messages.
// ────────────────────────────────────────────────────────────

pub(super) fn smart_commit_section(
    app: &Entity<KagiApp>,
    smart: &super::smart_commit::SmartCommitState,
    switches: &super::keyboard_nav::FocusSlots,
    controls: &super::settings_controls::SettingsControls,
    pointer_focus: Option<gpui::FocusHandle>,
    scroll: &gpui::ScrollHandle,
    cx: &gpui::App,
) -> impl IntoElement {
    let current = smart.model.clone();
    let models = smart.detected_models.clone();

    // ── Enable Smart Commit (LLM) toggle ──
    // Initial ON is a pending host-owned approval, not an enabled preference.
    // Re-enable after consent keeps the existing capability refresh behavior.
    let app_en = app.downgrade();
    let enabled_ctl = super::keyboard_nav::switch(
        "smart-commit-enabled",
        switches.get(5, cx),
        scroll.clone(),
        SharedString::from(Msg::SettingsSmartEnable.t()),
        SharedString::from(Msg::SettingsSmartEnableDesc.t()),
        smart.llm_enabled,
        move |on, window, cx| {
            let _ = app_en.update(cx, |app, cx| {
                app.request_settings_smart_enabled(on, window, cx)
            });
        },
    );

    // ── Provider selector (ADR-0099): Ollama / Claude Code / Codex ──
    // Chips mirror the model-picker styling below. CLI providers are only
    // selectable when detected on $PATH; undetected ones are shown disabled with
    // a "not found on PATH" hint.
    let provider = smart.provider;
    let provider_chips =
        super::settings_controls::provider_chips(app, smart, controls, pointer_focus.clone(), cx)
            .into_any_element();

    // ── Prominent warning when a CLI provider is selected (ADR-0099) ──
    // The user explicitly asked for an unmissable warning about cost / quota /
    // privacy. Rendered in warning colour with a bordered block.
    let cli_warning: Option<AnyElement> = match provider {
        super::smart_commit::SmartProvider::Cli(p) => {
            let name = p.display_name();
            let bin = p.binary();
            let lines = i18n::smart_cli_warning_lines(name, bin);
            let mut block = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(theme().color_warning))
                .bg(rgb(theme().bg_base))
                .text_sm()
                .text_color(rgb(theme().color_warning))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(SharedString::from(i18n::smart_cli_warning_title(name))),
                );
            for l in lines {
                block = block.child(div().child(SharedString::from(format!("• {}", l))));
            }
            Some(block.into_any_element())
        }
        super::smart_commit::SmartProvider::Ollama => None,
    };

    let control: AnyElement = if models.is_empty() {
        let note = match &current {
            Some(m) => i18n::smart_model_switch_note(m),
            None => Msg::SettingsSmartNoModels.t().to_string(),
        };
        div()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(note))
            .into_any_element()
    } else {
        super::settings_controls::model_chips(app, smart, controls, pointer_focus, cx)
            .into_any_element()
    };

    // The Ollama model picker is only relevant when Ollama is the provider.
    let show_model_row = matches!(provider, super::smart_commit::SmartProvider::Ollama);

    let mut section = settings_group("settings-smart-group")
        .child(setting_row_with_help(
            true,
            "settings-smart-enable-help",
            Msg::SettingsSmartEnable,
            Msg::SettingsSmartEnableSummary,
            Msg::SettingsSmartEnableDesc,
            enabled_ctl,
        ))
        .child(setting_row_with_help(
            false,
            "settings-provider-help",
            Msg::SettingsSmartProvider,
            Msg::SettingsSmartProviderSummary,
            Msg::SettingsSmartProviderDesc,
            provider_chips,
        ));

    if let Some(warning) = cli_warning {
        section = section.child(div().py_2().child(warning));
    }

    if show_model_row {
        section = section.child(setting_row(
            false,
            SharedString::from(Msg::SettingsSmartModel.t()),
            SharedString::from(Msg::SettingsSmartModelDesc.t()),
            control,
        ));
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(
            "settings-smart-heading",
            Msg::SettingsSmartCommit.t().into(),
        ))
        .child(section)
}

// ────────────────────────────────────────────────────────────
// Analyze ignore (ADR-0119): edit the gitignore-syntax exclude list used by
// the Analyze view, persisted to the sibling `analyze_ignore` file.
// ────────────────────────────────────────────────────────────

pub(super) fn analyze_ignore_section(
    app: &Entity<KagiApp>,
    input: Option<Entity<InputState>>,
    dirty: bool,
    error: Option<&str>,
) -> impl IntoElement {
    // Multiline editor (or a fallback note pre-window / headless).
    let editor: AnyElement = match input.clone() {
        Some(state) => Input::new(&state)
            .context_menu(kagi_ui_core::input::context_menu(&state, false))
            .aria_label(Msg::A11yAnalyzeIgnorePatterns.t())
            .appearance(true)
            .bordered(true)
            .into_any_element(),
        None => div()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(Msg::SettingsAnalyzeIgnoreDesc.t()))
            .into_any_element(),
    };

    // ── Save: write the editor's current text to the analyze_ignore file ──
    let save_btn = {
        let app = app.downgrade();
        Button::new("analyze-ignore-save")
            .label(Msg::SettingsAnalyzeIgnoreSave.t())
            .primary()
            .small()
            .disabled(!dirty)
            .on_click(move |_, window, cx| {
                let _ = app.update(cx, |app, cx| app.save_analyze_ignore(false, window, cx));
            })
    };
    let save_btn =
        super::button_style::named_button(save_btn, Msg::SettingsAnalyzeIgnoreSave.t(), None);

    // ── Reset: restore the documented defaults (both editor + file) ──
    let reset_btn = {
        let app = app.downgrade();
        Button::new("analyze-ignore-reset")
            .label(Msg::SettingsAnalyzeIgnoreReset.t())
            .outline()
            .small()
            .on_click(move |_, window, cx| {
                let _ = app.update(cx, |app, cx| app.request_analyze_reset(window, cx));
            })
    };
    let reset_btn =
        super::button_style::named_button(reset_btn, Msg::SettingsAnalyzeIgnoreReset.t(), None);

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_header(
            "settings-analyze-heading",
            SharedString::from(Msg::SettingsAnalyze.t()),
        ))
        .child(
            settings_group("settings-analyze-group").child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .py_2()
                    .child(
                        div()
                            .text_color(rgb(theme().text_main))
                            .child(super::e2e::measure_inside("settings-analyze-label"))
                            .child(SharedString::from(Msg::SettingsAnalyzeIgnore.t())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .id("settings-analyze-help")
                            .text_color(rgb(theme().text_sub))
                            .child(super::e2e::measure_inside("settings-analyze-description"))
                            .child(SharedString::from(Msg::SettingsAnalyzeIgnoreSummary.t()))
                            .hover_tooltip_builder("settings-analyze-hover-help", |window, cx| {
                                #[cfg(feature = "gui-e2e")]
                                record_settings_help(
                                    "settings-analyze-help",
                                    Msg::SettingsAnalyzeIgnoreDesc.t(),
                                );
                                gpui_component::tooltip::Tooltip::new(
                                    Msg::SettingsAnalyzeIgnoreDesc.t(),
                                )
                                .build(window, cx)
                            }),
                    )
                    .child(super::e2e::measure_control(
                        "settings-analyze-editor",
                        editor,
                    ))
                    .child(super::e2e::measure_control(
                        "settings-analyze-help-text",
                        div()
                            .text_sm()
                            .text_color(rgb(theme().text_sub))
                            .child(Msg::SettingsAnalyzeIgnoreDesc.t()),
                    ))
                    .when(dirty, |el| {
                        el.child(super::e2e::measure_control(
                            "settings-analyze-dirty",
                            div()
                                .text_sm()
                                .text_color(rgb(theme().text_sub))
                                .child(Msg::SettingsAnalyzeIgnoreUnsaved.t()),
                        ))
                    })
                    .when(!dirty, |el| {
                        el.child(super::e2e::measure_control(
                            "settings-analyze-saved",
                            div()
                                .text_sm()
                                .text_color(rgb(theme().text_sub))
                                .child(Msg::SettingsAnalyzeIgnoreClean.t()),
                        ))
                    })
                    .when_some(error, |el, error| {
                        el.child(super::e2e::measure_control(
                            "settings-analyze-error",
                            div()
                                .text_sm()
                                .text_color(rgb(theme().color_blocker))
                                .child(SharedString::from(error.to_string())),
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .justify_end()
                            .child(super::e2e::measure_control(
                                "settings-analyze-reset",
                                reset_btn,
                            ))
                            .child(super::e2e::measure_control(
                                "settings-analyze-save",
                                save_btn,
                            )),
                    ),
            ),
        )
}
